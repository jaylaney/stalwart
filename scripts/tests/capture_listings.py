#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Capture the libtest listings the inventory gate reads (`--listing`, `--ignored-listing`).

Usage:
  capture_listings.py --out DIR [CONFIGURATION ...]

Writes DIR/list/<configuration>.txt and DIR/ign/<configuration>--ignored-only.txt, each the
`-- --list` (or `-- --ignored --list`) output of one CI step's command, stdout and stderr
together, in the baseline's capture format. With no CONFIGURATION it captures all of them.

The commands mirror `.github/workflows/test.yml`'s steps, as the configuration names do
(`test.yml__<job>__<step>`). Keep them in step: a change to a step's command needs the same
change here. The workspace unit step's features come from `UNIT_TEST_FEATURES`, which
test.yml sets once for both jobs that use it; the variable is required. Tests crate steps
list with `--lib --bins --tests`, as the baseline's captures did, and filtered steps also
capture the unfiltered listing of the same binaries. Run it from any directory of the
repository. Needs Python 3.9 or later and no packages.
"""
import argparse
import datetime
import os
from pathlib import Path
import subprocess
import sys

REPO = Path(__file__).resolve().parents[2]
BASE_ENV = {"STORE": "RocksDb", "RUST_MIN_STACK": "16777216"}
SPAM = ("https://github.com/stalwartlabs/spam-filter/releases/download/v3.0.3/"
        "spam-filter-rules.json.gz")
TESTS = ["cargo", "test", "-p", "tests", "--lib", "--bins", "--tests"]


def tests_step(filter_, env=None, exact=False):
    tail = ["--exact", "--nocapture"] if exact else ["--nocapture"]
    return dict(cmd=TESTS + [filter_, "--"] + tail, unfiltered=TESTS + ["--"] + tail,
                env=env or {})


def plain(cmd):
    return dict(cmd=cmd + ["--"], unfiltered=None, env={})


def configurations(unit_features):
    workspace = ["cargo", "test", "--workspace", "--exclude", "tests", "--exclude", "scim",
                 "--exclude", "scim-proto", "--lib", "--bins", "--features", unit_features]
    key = {"ZA_KEY_ACCOUNTS": "1"}
    return {
        "test.yml__test__directory-tests": tests_step("directory"),
        "test.yml__test__imap-tests": tests_step("imap"),
        "test.yml__test__jmap-tests": tests_step("jmap"),
        "test.yml__test__smtp-tests": tests_step("smtp", {"SPAM_RULES_URL": SPAM}),
        "test.yml__unit-tests__workspace-unit-tests": plain(workspace),
        # Ruling 53 and the http test_mode step: the two selection moves' targets.
        "test.yml__unit-tests__smtp-unit-tests-without-enterprise": plain(
            ["cargo", "test", "-p", "smtp", "--lib", "--bins", "--features", "test_mode"]),
        "test.yml__unit-tests__http-unit-tests": plain(
            ["cargo", "test", "-p", "http@1.0.1", "--lib", "--bins", "--features",
             "test_mode,enterprise"]),
        "test.yml__upstream-suites__automation-tests": tests_step("automation::automation_tests"),
        "test.yml__upstream-suites__blob-tests": tests_step("store::blob::blob_tests"),
        "test.yml__upstream-suites__caldav-query-expansion-tests": tests_step(
            "webdav::cal_query::"),
        "test.yml__upstream-suites__lookup-tests": tests_step("store::lookup::lookup_tests"),
        "test.yml__upstream-suites__search-tests": tests_step("store::search_tests"),
        "test.yml__upstream-suites__store-tests": tests_step("store::store_tests"),
        "test.yml__upstream-suites__system-tests": tests_step("system::system_tests"),
        "test.yml__upstream-suites__telemetry-tests": tests_step("telemetry::telemetry_tests"),
        "test.yml__zero-access__caldav-tests-key-accounts": tests_step("webdav::webdav_tests",
                                                                       key),
        "test.yml__zero-access__caldav-tests-plain-accounts": tests_step("webdav::webdav_tests"),
        "test.yml__zero-access__imap-tests-key-accounts": tests_step("imap::imap_tests", key),
        "test.yml__zero-access__imap-tests-plain-accounts-push-only": tests_step(
            "imap::imap_tests"),
        "test.yml__zero-access__jmap-tests-key-accounts": tests_step("jmap::jmap_tests", key),
        "test.yml__zero-access__jmap-tests-plain-accounts-push-only": tests_step(
            "jmap::jmap_tests"),
        "test.yml__zero-access__vault-and-groupware-unit-tests": dict(
            cmd=["cargo", "test", "-p", "vault", "-p", "groupware", "--features",
                 "vault/test_mode", "--lib", "--bins", "--tests", "--", "--nocapture"],
            unfiltered=None, env={}),
        "test.yml__zero-access__zero-access-leak-scanner-unit-test": tests_step(
            "za::leak::tests::try_archive_short_values_are_not_archives", exact=True),
        "test.yml__zero-access__zero-access-tests": tests_step("za::za_tests"),
        "test.yml__zero-access__zero-access-tests-filesystem-blob-store": tests_step(
            "za::za_tests", {"BLOB_STORE": "FileSystem"}),
        # Package captures whose ignored-only listings the baseline requires.
        "exists__nlp__default": plain(["cargo", "test", "-p", "nlp", "--lib", "--bins",
                                       "--tests"]),
        "exists__spam-filter__default": plain(["cargo", "test", "-p", "spam-filter", "--lib",
                                               "--bins", "--tests"]),
        "exists__tests__default": plain(list(TESTS)),
    }


def now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run(cmd, extra):
    env = dict(os.environ)
    for key in ("ZA_KEY_ACCOUNTS", "BLOB_STORE", "SPAM_RULES_URL", "RUSTFLAGS"):
        env.pop(key, None)
    env.update(BASE_ENV)
    env.update(extra)
    result = subprocess.run(cmd, cwd=REPO, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True)
    return result.returncode, result.stdout


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
