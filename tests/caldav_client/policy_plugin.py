# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Enforce the complete pinned suite and reviewed exceptional outcomes."""
import json
import os
from pathlib import Path

import pytest

from runner import HERE, check_outcomes

_outcomes = {}
_keys = {}
_records = {}


def test_key(item):
    if item.path.name == "test_flows.py":
        kind = "flows/"
    elif item.cls and item.cls.__name__.startswith("TestSchedulingForServerStalwart"):
        kind = "scheduling/"
    elif item.cls and item.cls.__name__.startswith("TestForServerStalwart"):
        kind = "calendar/"
    else:
        raise pytest.UsageError("Unexpected class/file in pinned client suite")
    return kind + item.nodeid.split("::")[-1]


@pytest.hookimpl(trylast=True)
def pytest_collection_modifyitems(session, config, items):
    if any(config.getoption(option) for option in ("keyword", "markexpr", "collectonly")):
        raise pytest.UsageError("Filtering or collection-only is forbidden for the pinned suite")
    expected = set(json.loads((HERE / "collection.json").read_text()))
    _keys.update({item.nodeid: test_key(item) for item in items})
    if len(items) != len(expected) or set(_keys.values()) != expected:
        raise pytest.UsageError("Incomplete or unexpected pinned class/strict-flow collection")
    policies = json.loads(Path(os.environ["CALDAV_CI_POLICY"]).read_text())
    policy = policies[os.environ["CALDAV_CI_MODE"]]
    # Validate exception policy before applying any marker or performing server I/O.
    check_outcomes({}, policy["outcomes"])
    from caldav.compatibility_hints import FeatureSet
    features = {name: record["value"] for name, record in policy["features"].items()}
    feature_set = FeatureSet(features)
    for name in features:
        feature_set.find_feature(name)  # Unknown names must fail before any server request.
    for item in items:
        record = policy["outcomes"].get(_keys[item.nodeid], {})
        if record.get("outcome") == "xfailed":
            failure = record.get("failure", {})
            if not failure.get("file") or not isinstance(failure.get("line"), int):
                raise pytest.UsageError("Expected failure must name its observed source location")
            _records[item.nodeid] = record


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item, call):
    result = yield
    report = result.get_result()
    record = _records.get(item.nodeid)
    # Never exempt setup or teardown, even if they raise the recorded exception type.
    if report.when != "call" or record is None:
        return
    if report.passed:
        report.outcome = "failed"
        report.longrepr = "XPASS: stale compatibility policy: " + record["reason"]
    elif report.failed and call.excinfo is not None:
        exception_types = {"AssertionError": AssertionError,
                           "DeprecationWarning": DeprecationWarning}
        entry = call.excinfo.traceback[-1]
        failure = record["failure"]
        location_matches = (str(entry.path).endswith("/" + failure["file"])
                            and entry.lineno + 1 == failure["line"])
        if call.excinfo.type is exception_types[record["exception"]] and location_matches:
            report.outcome = "skipped"
            report.longrepr = (str(entry.path), entry.lineno + 1, record["reason"])
            report.wasxfail = record["reason"]


def pytest_deselected(items):
    if items:
        raise pytest.UsageError("Deselection is forbidden for the pinned suite")


def pytest_runtest_logreport(report):
    if report.when == "call" or report.failed or report.skipped:
        key = _keys[report.nodeid]
        outcome = report.outcome
        if getattr(report, "wasxfail", None):
            outcome = "xfailed" if report.skipped else "xpassed"
        if report.failed or _outcomes.get(key) not in ("failed", "skipped"):
            _outcomes[key] = outcome


def pytest_sessionfinish(session, exitstatus):
    policy = json.loads(Path(os.environ["CALDAV_CI_POLICY"]).read_text())
    expected = set(json.loads((HERE / "collection.json").read_text()))
    problems = check_outcomes(_outcomes, policy[os.environ["CALDAV_CI_MODE"]]["outcomes"])
    if set(_outcomes) != expected:
        problems.append("Pinned test reports missing or unexpected")
    if problems:
        terminal = session.config.pluginmanager.get_plugin("terminalreporter")
        for problem in problems:
            terminal.write_line("COMPATIBILITY POLICY: " + problem, red=True)
        session.exitstatus = pytest.ExitCode.TESTS_FAILED
