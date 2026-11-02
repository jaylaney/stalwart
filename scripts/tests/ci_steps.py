#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""The CI steps the C10 gates measure: one table, and the check that ties it to test.yml.

Usage:
  ci_steps.py [--workflow PATH]

`STEPS` holds every `cargo test` step of test.yml's test, unit-tests, upstream-suites and
zero-access jobs: its command as test.yml writes it, the environment it adds, its `if`
condition, its rerun, its gate configuration and its coverage-net step. `capture_listings.py`
derives each listing command from it (`listing_command`), and `coverage_net.py` each of its
tests-crate steps (`net_command`), so the gates capture and measure what CI runs.

`check_workflow` compares the table with `.github/workflows/test.yml` and returns one line per
difference: a step missing or renamed, a changed command (a narrowed filter, a dropped feature),
a changed environment, a changed `if`, a changed rerun, a step that may fail without failing its
job (`continue-on-error`), a `cargo test` step the table does not hold, or a gates job that runs
the net or the capture on fewer steps. `test_gates.py` runs it on the committed workflow (it must
find nothing) and on edited copies. Run as a command, it checks the committed workflow, or PATH,
and exits 1 if it finds a difference.

A change to one of those steps in test.yml therefore needs the same change here, and with it the
gates capture and measure the new command. Needs Python 3.9 or later and no packages: test.yml is
read by `load_yaml`, a parser of the YAML subset the workflow uses, which refuses anything else.
"""
import argparse
import json
from pathlib import Path
import re
import shlex
import sys

REPO = Path(__file__).resolve().parents[2]
WORKFLOW = REPO / ".github" / "workflows" / "test.yml"

# Every gated job sets these for all its steps.
BASE_ENV = {"STORE": "RocksDb", "RUST_MIN_STACK": "16777216"}
SPAM_URL = ("https://github.com/stalwartlabs/spam-filter/releases/download/v3.0.3/"
            "spam-filter-rules.json.gz")
# The workspace unit step's features: test.yml's workflow-level variable, which the gates job's
# listing capture reads from its environment too.
UNIT_FEATURES = "UNIT_TEST_FEATURES"
ALWAYS = "${{ !cancelled() }}"
PUSH_ONLY = "${{ !cancelled() && github.event_name == 'push' }}"
NOT_ON_PUSH = "github.event_name != 'push'"
# The SMTP step's second run: the failed tests by name (test.yml's awk collects `$failed`).
BY_NAME_RETRY = "cargo test -p tests -- --exact --nocapture $failed"

# The jobs that hold the gated steps: their `if`, and the environment each sets for its steps.
JOBS = {
    "test": dict(when=NOT_ON_PUSH, env=BASE_ENV),
    "unit-tests": dict(when=None, env=BASE_ENV),
    "upstream-suites": dict(when=NOT_ON_PUSH, env=BASE_ENV),
    "zero-access": dict(when=None, env=BASE_ENV),
}

# The gates job, and the two commands that must run the whole net and every capture.
GATES_JOB = "gates"
GATES_WHEN = "github.event_name == 'workflow_dispatch' || github.event_name == 'schedule'"
GATES_COMMANDS = {
    "Coverage net (19 steps)": 'python3 -I scripts/tests/coverage_net.py --out "$GATES/net"',
    "Capture CI step listings":
        'python3 -I scripts/tests/capture_listings.py --out "$GATES/listings"',
}

# Keys a gated step may have. Anything else (continue-on-error, working-directory, ...) can change
# what the step runs or whether its failure counts.
STEP_KEYS = {"name", "if", "run", "env", "shell"}
JOB_FORBIDDEN = {"continue-on-error", "defaults", "strategy", "container"}


class Step:
    """One gated CI step.

    config   the gates' configuration name (`test.yml__<job>__<step>`), or None when the
             inventory gate has no configuration for it
    command  test.yml's first `cargo test` command of the step, as written
    env      the variables the step adds to its job's
    when     the step's `if`, or None
    rerun    None, "same" (the step runs its command once more after a failure) or "by-name"
             (it reruns its failed tests by name, BY_NAME_RETRY)
    net      the coverage-net step that measures this command, or None
    capture  whether capture_listings.py captures it (a configuration of the baseline)
    """

    def __init__(self, config, job, name, command, env=None, when=ALWAYS, rerun=None,
                 net=None, capture=True, note=""):
        self.config, self.job, self.name, self.command = config, job, name, command
        self.env = env or {}
        self.when, self.rerun, self.net, self.capture, self.note = when, rerun, net, capture, note
        self.argv = shlex.split(command)


KEY = {"ZA_KEY_ACCOUNTS": "1"}

STEPS = [
    # test
    Step("test.yml__test__directory-tests", "test", "Directory Tests",
         "cargo test -p tests directory -- --nocapture", when=None),
    Step("test.yml__test__smtp-tests", "test", "SMTP Tests",
         "cargo test -p tests smtp -- --nocapture", env={"SPAM_RULES_URL": SPAM_URL},
         rerun="by-name", net="smtp"),
    Step("test.yml__test__imap-tests", "test", "IMAP Tests",
         "cargo test -p tests imap -- --nocapture"),
    Step("test.yml__test__jmap-tests", "test", "JMAP Tests",
         "cargo test -p tests jmap -- --nocapture", rerun="same"),
    # unit-tests
    Step("test.yml__unit-tests__workspace-unit-tests", "unit-tests", "Workspace Unit Tests",
         "cargo test --workspace --exclude tests --exclude scim --exclude scim-proto "
         '--lib --bins --features "$UNIT_TEST_FEATURES"', when=None),
    # Ruling 53 and the http test_mode step: the two selection moves' targets.
    Step("test.yml__unit-tests__smtp-unit-tests-without-enterprise", "unit-tests",
         "SMTP Unit Tests (without enterprise)",
         "cargo test -p smtp --lib --bins --features test_mode", when=None),
    Step("test.yml__unit-tests__http-unit-tests", "unit-tests", "HTTP Unit Tests",
         "cargo test -p http@1.0.1 --lib --bins --features test_mode,enterprise", when=None),
    # upstream-suites
    Step("test.yml__upstream-suites__caldav-query-expansion-tests", "upstream-suites",
         "CalDAV Query Expansion Tests", "cargo test -p tests 'webdav::cal_query::' -- --nocapture",
         when=None, net="cal-query-tests"),
    Step("test.yml__upstream-suites__store-tests", "upstream-suites", "Store Tests",
         "cargo test -p tests store::store_tests -- --nocapture", net="store-tests"),
    Step("test.yml__upstream-suites__blob-tests", "upstream-suites", "Blob Tests",
         "cargo test -p tests store::blob::blob_tests -- --nocapture", net="blob-tests"),
    Step("test.yml__upstream-suites__lookup-tests", "upstream-suites", "Lookup Tests",
         "cargo test -p tests store::lookup::lookup_tests -- --nocapture", net="lookup-tests"),
    Step("test.yml__upstream-suites__search-tests", "upstream-suites", "Search Tests",
         "cargo test -p tests store::search_tests -- --nocapture", net="search-tests"),
    Step("test.yml__upstream-suites__telemetry-tests", "upstream-suites", "Telemetry Tests",
         "cargo test -p tests telemetry::telemetry_tests -- --nocapture",
         net="telemetry-tests"),
    Step("test.yml__upstream-suites__system-tests", "upstream-suites", "System Tests",
         "cargo test -p tests system::system_tests -- --nocapture", net="system-tests"),
    Step("test.yml__upstream-suites__automation-tests", "upstream-suites", "Automation Tests",
         "cargo test -p tests automation::automation_tests -- --nocapture",
         net="automation-tests"),
    # zero-access
    Step("test.yml__zero-access__vault-and-groupware-unit-tests", "zero-access",
         "Vault and Groupware Unit Tests",
         "cargo test -p vault -p groupware --features vault/test_mode -- --nocapture", when=None),
    Step("test.yml__zero-access__zero-access-leak-scanner-unit-test", "zero-access",
         "Zero-access Leak Scanner Unit Test",
         "cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives "
         "-- --exact --nocapture", net="leak-unit-test"),
    Step("test.yml__zero-access__zero-access-tests", "zero-access", "Zero-access Tests",
         "cargo test -p tests za::za_tests -- --nocapture", rerun="same", net="za-tests"),
    Step("test.yml__zero-access__zero-access-tests-filesystem-blob-store", "zero-access",
         "Zero-access Tests (filesystem blob store)",
         "cargo test -p tests za::za_tests -- --nocapture", env={"BLOB_STORE": "FileSystem"},
         rerun="same", net="za-tests-fs-blob"),
    Step(None, "zero-access", "Zero-access Cache Swap Tests",
         "cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture",
         capture=False,
         note="new at 1.0 (ruling 20): no baseline configuration or net step; its test is "
              "listed in the tests binary's unfiltered sections"),
    Step("test.yml__zero-access__caldav-tests-plain-accounts", "zero-access",
         "CalDAV Tests (plain accounts)", "cargo test -p tests webdav::webdav_tests -- --nocapture",
         rerun="same", net="webdav-plain"),
    Step("test.yml__zero-access__caldav-tests-key-accounts", "zero-access",
         "CalDAV Tests (key accounts)", "cargo test -p tests webdav::webdav_tests -- --nocapture",
         env=KEY, rerun="same", net="webdav-key"),
    Step("test.yml__zero-access__imap-tests-plain-accounts-push-only", "zero-access",
         "IMAP Tests (plain accounts)", "cargo test -p tests imap::imap_tests -- --nocapture",
         when=PUSH_ONLY, net="imap-plain"),
    Step("test.yml__zero-access__jmap-tests-plain-accounts-push-only", "zero-access",
         "JMAP Tests (plain accounts)", "cargo test -p tests jmap::jmap_tests -- --nocapture",
         when=PUSH_ONLY, rerun="same", net="jmap-plain"),
    Step("test.yml__zero-access__imap-tests-key-accounts", "zero-access",
         "IMAP Tests (key accounts)", "cargo test -p tests imap::imap_tests -- --nocapture",
         env=KEY, net="imap-key"),
    Step("test.yml__zero-access__jmap-tests-key-accounts", "zero-access",
         "JMAP Tests (key accounts)", "cargo test -p tests jmap::jmap_tests -- --nocapture",
         env=KEY, net="jmap-key"),
]

TESTS_CRATE = ["cargo", "test", "-p", "tests"]
# The baseline captured the tests crate and the packages with every test target, no doctests.
LISTING_TARGETS = ["--lib", "--bins", "--tests"]


def expand(argv, unit_features):
    """The command with `$UNIT_TEST_FEATURES` replaced by its value."""
    return [unit_features if arg == "$" + UNIT_FEATURES else arg for arg in argv]


def listing_command(step, unit_features):
    """`(cmd, unfiltered)`: the step's listing command, before `--list`, and for a filtered
    tests-crate step the same binaries' unfiltered command (else None)."""
    argv = expand(step.argv, unit_features)
    if argv[:4] == TESTS_CRATE:
        rest = argv[4:]
        split = rest.index("--")
        tail = rest[split + 1:]
        cmd = TESTS_CRATE + LISTING_TARGETS + rest
        unfiltered = TESTS_CRATE + LISTING_TARGETS + ["--"] + tail if rest[:split] else None
        return cmd, unfiltered
    head, tail = (argv[:argv.index("--")], argv[argv.index("--"):]) if "--" in argv else (
        argv, ["--"])
    if "--lib" not in head:
        head = head + LISTING_TARGETS
    return head + tail, None


def net_command(step, llvm_test):
    """The coverage-net command of a tests-crate step: `cargo test` becomes `llvm_test`."""
    if step.argv[:2] != ["cargo", "test"]:
        raise ValueError(f"{step.name}: not a cargo test command")
    return list(llvm_test) + step.argv[2:]


def net_steps():
    return {step.net: step for step in STEPS if step.net}


def captured_steps():
    return {step.config: step for step in STEPS if step.capture}


# ============================================================ the YAML subset test.yml uses
class YamlSubsetError(ValueError):
    pass


_KEY = re.compile(r"""^([A-Za-z0-9_.\-]+|"[^"]*"|'[^']*'):(?:[ ]+(.*))?$""")


def _indent(line):
    return len(line) - len(line.lstrip(" "))


def _scalar(text, where):
    text = text.strip()
    for quote in ('"', "'"):
        if text.startswith(quote):
            end, index = None, 1
            while index < len(text):
                if quote == '"' and text[index] == "\\":
                    index += 2
                    continue
                if text[index] == quote:
                    if quote == "'" and text[index + 1:index + 2] == "'":
                        index += 2
                        continue
                    end = index
                    break
                index += 1
            if end is None:
                raise YamlSubsetError(f"{where}: unterminated quoted scalar")
            rest = text[end + 1:].strip()
            if rest and not rest.startswith("#"):
                raise YamlSubsetError(f"{where}: text after a quoted scalar")
            body = text[:end + 1]
            return json.loads(body) if quote == '"' else body[1:-1].replace("''", "'")
    cut = re.search(r"\s#", text)
    if cut:
        text = text[:cut.start()].rstrip()
    if text.startswith(("&", "*", "!", "{", "|", ">", "%", "@", "`")):
        raise YamlSubsetError(f"{where}: unsupported scalar {text[:20]!r}")
    return text


class _Parser:
    def __init__(self, text):
        if "\t" in text:
            raise YamlSubsetError("tab characters are not supported")
        self.lines = [line.rstrip() for line in text.split("\n")]
        self.i = 0

    def peek(self):
        while self.i < len(self.lines):
            stripped = self.lines[self.i].strip()
            if stripped and not stripped.startswith("#"):
                if stripped in ("---", "..."):
                    raise YamlSubsetError(f"line {self.i + 1}: documents are not supported")
                return _indent(self.lines[self.i]), stripped
            self.i += 1
        return None

    def block(self, indent):
        ind, content = self.peek()
        if ind != indent:
            raise YamlSubsetError(f"line {self.i + 1}: unexpected indentation")
        if content == "-" or content.startswith("- "):
            return self.sequence(indent)
        return self.mapping(indent)

    def mapping(self, indent):
        result = {}
        while True:
            peeked = self.peek()
            if peeked is None or peeked[0] < indent:
                return result
            ind, content = peeked
            where = f"line {self.i + 1}"
            if ind > indent or content == "-" or content.startswith("- "):
                raise YamlSubsetError(f"{where}: unexpected indentation or sequence item")
            match = _KEY.match(content)
            if not match:
                raise YamlSubsetError(f"{where}: not a mapping entry: {content[:40]!r}")
            key = _scalar(match.group(1), where)
            if key in result:
                raise YamlSubsetError(f"{where}: duplicate key {key!r}")
            self.i += 1
            result[key] = self.value(match.group(2) or "", indent, where)

    def value(self, rest, indent, where):
        stripped = rest.strip()
        if stripped.startswith("#"):
            stripped = ""
        if stripped in ("|", "|-", ">", ">-"):
            return self.block_scalar(stripped, indent)
        if stripped:
            return _scalar(stripped, where)
        peeked = self.peek()
        if peeked is None:
            return None
        if peeked[0] > indent:
            return self.block(peeked[0])
        if peeked[0] == indent and (peeked[1] == "-" or peeked[1].startswith("- ")):
            return self.sequence(indent)
        return None

    def sequence(self, indent):
        items = []
        while True:
            peeked = self.peek()
            if peeked is None or peeked[0] < indent:
                return items
            ind, content = peeked
            where = f"line {self.i + 1}"
            if ind > indent:
                raise YamlSubsetError(f"{where}: unexpected indentation")
            if not (content == "-" or content.startswith("- ")):
                return items
            rest = content[1:].strip()
            if not rest:
                self.i += 1
                peeked = self.peek()
                if peeked is None or peeked[0] <= indent:
                    raise YamlSubsetError(f"{where}: empty sequence item")
                items.append(self.block(peeked[0]))
            elif _KEY.match(rest):
                # The item's mapping starts on the dash's line: read it as if the dash were
                # spaces, at the indentation of its first key.
                self.lines[self.i] = " " * (indent + 2) + rest
                items.append(self.mapping(indent + 2))
            else:
                self.i += 1
                items.append(_scalar(rest, where))

    def block_scalar(self, style, indent):
        collected = []
        while self.i < len(self.lines):
            line = self.lines[self.i]
            if line.strip() and _indent(line) <= indent:
                break
            collected.append(line)
            self.i += 1
        while collected and not collected[-1].strip():
            collected.pop()
        if not collected:
            return ""
        block = min(_indent(line) for line in collected if line.strip())
        lines = [line[block:] if line.strip() else "" for line in collected]
        if style.startswith("|"):
            text = "\n".join(lines)
        else:
            paragraphs, current = [], []
            for line in lines:
                if line:
                    current.append(line)
                else:
                    paragraphs.append(" ".join(current))
                    current = []
            paragraphs.append(" ".join(current))
            text = "\n".join(paragraphs)
        return text if style.endswith("-") else text + "\n"


def load_yaml(text):
    """test.yml as nested dicts, lists and strings (every scalar stays a string)."""
    parser = _Parser(text)
    if parser.peek() is None:
        return None
    value = parser.block(parser.peek()[0])
    if parser.peek() is not None:
        raise YamlSubsetError(f"line {parser.i + 1}: unexpected content")
    return value


# ============================================================ the check
def cargo_invocations(run):
    """Every `cargo test` command in a step's `run` text, split as the shell would."""
    text = re.sub(r"\\\n[ \t]*", " ", run or "")
    found = []
    for match in re.finditer(r"\bcargo test\b[^|;&\n{}()]*", text):
        command = re.sub(r"\s+\d*>.*$", "", match.group(0)).strip()
        found.append(shlex.split(command))
    return found


def _condition(value):
    return " ".join(value.split()) if isinstance(value, str) else value


def _env(mapping):
    if mapping is None:
        return {}
    if not isinstance(mapping, dict):
        raise YamlSubsetError("an env block is not a mapping")
    return {key: "" if value is None else str(value) for key, value in mapping.items()}


def _env_problems(where, actual, expected):
    problems = []
    for key in sorted(set(actual) | set(expected)):
        if actual.get(key) != expected.get(key):
            problems.append(f"{where}: environment {key}: test.yml has {actual.get(key)!r}, "
                            f"the gates expect {expected.get(key)!r}")
    return problems


def check_workflow(text):
    """One line per difference between `STEPS` (and the gates job) and test.yml's text."""
    try:
        workflow = load_yaml(text)
    except YamlSubsetError as error:
        return [f"test.yml: {error}"]
    problems = []
    if not isinstance(workflow, dict) or not isinstance(workflow.get("jobs"), dict):
        return ["test.yml: no jobs"]
    workflow_env = _env(workflow.get("env"))
    if not workflow_env.get(UNIT_FEATURES, "").split():
        problems.append(f"test.yml: the workflow-level {UNIT_FEATURES} is missing or empty")
    jobs = workflow["jobs"]

    for name, spec in JOBS.items():
        job = jobs.get(name)
        if not isinstance(job, dict):
            problems.append(f"job {name}: missing")
            continue
        if _condition(job.get("if")) != _condition(spec["when"]):
            problems.append(f"job {name}: if is {job.get('if')!r}, the gates expect "
                            f"{spec['when']!r}")
        for key in sorted(JOB_FORBIDDEN & set(job)):
            problems.append(f"job {name}: has {key}")
        job_env = _env(job.get("env"))
        problems += _env_problems(f"job {name}", {k: v for k, v in job_env.items()},
                                  dict(spec["env"]))
        tracked = {step.name for step in STEPS if step.job == name}
        for item in job.get("steps") or []:
            if isinstance(item, dict) and cargo_invocations(item.get("run")) and (
                    item.get("name") not in tracked):
                problems.append(f"job {name}, step {item.get('name')!r}: runs cargo test but "
                                "is not in ci_steps.STEPS")

    for step in STEPS:
        where = f"job {step.job}, step {step.name!r}"
        job = jobs.get(step.job)
        if not isinstance(job, dict):
            continue
        matches = [item for item in job.get("steps") or []
                   if isinstance(item, dict) and item.get("name") == step.name]
        if len(matches) != 1:
            problems.append(f"{where}: {'missing (dropped or renamed)' if not matches else 'not unique'}")
            continue
        item = matches[0]
        for key in sorted(set(item) - STEP_KEYS):
            problems.append(f"{where}: has {key}")
        if item.get("shell") not in (None, "bash"):
            problems.append(f"{where}: shell is {item.get('shell')!r}")
        if _condition(item.get("if")) != _condition(step.when):
            problems.append(f"{where}: if is {item.get('if')!r}, the gates expect {step.when!r}")
        invocations = cargo_invocations(item.get("run"))
        if not invocations or invocations[0] != step.argv:
            shown = shlex.join(invocations[0]) if invocations else None
            problems.append(f"{where}: command is {shown!r}, the gates capture and measure "
                            f"{step.command!r}")
        expected_runs = {None: [step.argv], "same": [step.argv, step.argv],
                         "by-name": [step.argv, shlex.split(BY_NAME_RETRY)]}[step.rerun]
        if invocations and invocations[0] == step.argv and invocations != expected_runs:
            problems.append(f"{where}: its cargo test runs are "
                            f"{[shlex.join(argv) for argv in invocations]}, the gates expect "
                            f"{[shlex.join(argv) for argv in expected_runs]} (rerun "
                            f"{step.rerun!r})")
        step_env = _env(item.get("env"))
        job_env = _env(job.get("env"))
        effective = {**workflow_env, **job_env, **step_env}
        effective.pop(UNIT_FEATURES, None)
        problems += _env_problems(where, effective, {**BASE_ENV, **step.env})
        if "$" + UNIT_FEATURES in step.argv and (UNIT_FEATURES in job_env
                                                 or UNIT_FEATURES in step_env):
            problems.append(f"{where}: overrides the workflow-level {UNIT_FEATURES}, which the "
                            "gates' capture reads")

    gates = jobs.get(GATES_JOB)
    if not isinstance(gates, dict):
        problems.append(f"job {GATES_JOB}: missing")
    else:
        where = f"job {GATES_JOB}"
        if _condition(gates.get("if")) != _condition(GATES_WHEN):
            problems.append(f"{where}: if is {gates.get('if')!r}, expected {GATES_WHEN!r}")
        for key in sorted(JOB_FORBIDDEN & set(gates)):
            problems.append(f"{where}: has {key}")
        gates_env = _env(gates.get("env"))
        if UNIT_FEATURES in gates_env:
            problems.append(f"{where}: overrides the workflow-level {UNIT_FEATURES}")
        for key, value in BASE_ENV.items():
            if gates_env.get(key) != value:
                problems.append(f"{where}: environment {key} is {gates_env.get(key)!r}, "
                                f"expected {value!r}")
        steps = {item.get("name"): item for item in gates.get("steps") or []
                 if isinstance(item, dict)}
        for name, command in GATES_COMMANDS.items():
            item = steps.get(name)
            if item is None:
                problems.append(f"{where}, step {name!r}: missing")
                continue
            if (item.get("run") or "").strip() != command:
                problems.append(f"{where}, step {name!r}: runs {item.get('run')!r}, expected "
                                f"{command!r} (every step, every configuration)")
            for key in sorted(set(item) - {"name", "id", "if", "run", "shell"}):
                problems.append(f"{where}, step {name!r}: has {key}")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--workflow", type=Path, default=WORKFLOW)
    args = parser.parse_args()
    problems = check_workflow(args.workflow.read_text())
    for problem in problems:
        print("FAIL", problem)
    print(f"{len(problems)} difference(s) between ci_steps.STEPS and {args.workflow}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
