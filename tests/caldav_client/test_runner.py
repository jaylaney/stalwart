# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Offline regression tests for the CI harness's failure boundaries."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from unittest.mock import patch
import sys
import unittest

import runner


class RunnerTests(unittest.TestCase):
    def test_jmap_partial_error_is_fatal_and_does_not_disclose_response(self):
        response = {"methodResponses": [["x:Account/set", {
            "created": {"a": {"id": "1"}},
            "notCreated": {"b": {"description": "secret-password"}},
        }, "0"]]}
        with self.assertRaisesRegex(RuntimeError, "notCreated") as failure:
            runner.checked_jmap(response, "x:Account/set")
        self.assertNotIn("secret-password", str(failure.exception))

    def test_wrong_jmap_method_is_fatal(self):
        with self.assertRaises(RuntimeError):
            runner.checked_jmap({"methodResponses": [["error", {"type": "forbidden"}, "0"]]},
                                "x:Account/set")

    def test_stop_reaps_unresponsive_child(self):
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        runner.stop_child(child, timeout=0.01)
        self.assertIsNotNone(child.returncode)

    def test_unlisted_skip_fails_and_stale_xfail_fails(self):
        policy = {"testKnown": {"outcome": "skipped", "reason": "intentional"}}
        self.assertEqual(runner.check_outcomes({"testKnown": "skipped", "testOther": "passed"},
                                             policy), [])
        self.assertTrue(runner.check_outcomes({"testNew": "skipped"}, policy))
        self.assertTrue(runner.check_outcomes({"testKnown": "passed"}, policy))

    def test_strict_flows_cannot_be_added_to_exception_policy(self):
        with self.assertRaises(ValueError):
            runner.check_outcomes({"flows/test_invitation": "xfailed"}, {
                "flows/test_invitation": {"outcome": "xfailed", "exception": "AssertionError",
                                          "reason": "bad waiver", "source": "fixture"}})

    def test_policy_cannot_accept_an_actual_failure(self):
        with self.assertRaises(ValueError):
            runner.check_outcomes({"testBroken": "failed"},
                                  {"testBroken": {"outcome": "failed", "reason": "bug"}})

    def test_expected_call_xfail_cannot_hide_setup_teardown_or_different_assertion(self):
        cases = {
            "call": ("", "        assert False  # observed\n", "", 0),
            "teardown": ("", "        assert False  # observed\n",
                         "    def teardown_method(self):\n        assert False\n", 1),
            "setup": ("    def setup_method(self):\n        assert False\n",
                      "        assert False  # observed\n", "", 1),
            "different": ("", "        assert False  # different\n"
                          "        assert False  # observed\n", "", 1),
            "xpass": ("", "        pass  # observed\n", "", 1),
        }
        for name, (setup, body, teardown, expected_exit) in cases.items():
            with self.subTest(case=name), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                source = ("class TestForServerStalwartPlain:\n" + setup
                          + "    def testKnown(self):\n" + body + teardown)
                test = root / "test_sample.py"
                test.write_text(source)
                line = next(i for i, text in enumerate(source.splitlines(), 1)
                            if "# observed" in text)
                policy = {"plain": {"features": {}, "outcomes": {"calendar/testKnown": {
                    "outcome": "xfailed", "exception": "AssertionError", "reason": "fixture",
                    "source": "fixture", "failure": {"file": "test_sample.py", "line": line}}}}}
                (root / "policy.json").write_text(json.dumps(policy))
                (root / "collection.json").write_text('["calendar/testKnown"]')
                (root / "conftest.py").write_text(
                    "from pathlib import Path\nimport policy_plugin\n"
                    "policy_plugin.HERE = Path(__file__).parent\n")
                env = {**os.environ, "PYTHONPATH": str(runner.HERE),
                       "CALDAV_CI_MODE": "plain", "CALDAV_CI_POLICY": str(root / "policy.json"),
                       "PYTEST_DISABLE_PLUGIN_AUTOLOAD": "1", "PYTEST_ADDOPTS": ""}
                result = subprocess.run([sys.executable, "-m", "pytest", "-p", "policy_plugin",
                                         "-q", str(test)], env=env, cwd=root,
                                        text=True, capture_output=True)
                self.assertEqual(result.returncode, expected_exit, result.stdout + result.stderr)

    def test_dirty_source_and_untracked_yaml_never_enter_executed_tree(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            source.mkdir()
            subprocess.run(["git", "init", "-q", str(source)], check=True)
            (source / "test.py").write_text("assert True\n")
            subprocess.run(["git", "-C", str(source), "add", "test.py"], check=True)
            subprocess.run(["git", "-C", str(source), "-c", "user.name=Fixture",
                            "-c", "user.email=fixture@client-ci.test", "commit", "-qm", "fixture"],
                           check=True)
            pin = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"],
                                          text=True).strip()
            (source / "test.py").write_text("assert False\n")
            (source / "caldav_test_servers.yaml").write_text("malicious: external-server\n")
            with patch.object(runner, "PIN", pin):
                runner.materialize_source(source, root / "executed")
            self.assertEqual((root / "executed/test.py").read_text(), "assert True\n")
            self.assertFalse((root / "executed/caldav_test_servers.yaml").exists())

    def test_pytest_keyword_and_marker_filters_are_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            test = root / "test_sample.py"
            sentinel = root / "ran"
            test.write_text("def test_sample():\n    open(" + repr(str(sentinel))
                            + ", 'w').close()\n")
            env = {**os.environ, "PYTHONPATH": str(runner.HERE),
                   "CALDAV_CI_MODE": "plain",
                   "CALDAV_CI_POLICY": str(runner.HERE / "compatibility.json"),
                   "PYTEST_DISABLE_PLUGIN_AUTOLOAD": "1", "PYTEST_ADDOPTS": ""}
            for flag in ("-k", "-m"):
                with self.subTest(flag=flag):
                    result = subprocess.run([sys.executable, "-m", "pytest", "-p", "policy_plugin",
                                             flag, "test", str(test)], env=env, cwd=root,
                                            text=True, capture_output=True)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("forbidden", result.stdout + result.stderr)
                    self.assertFalse(sentinel.exists())

    def test_empty_or_incomplete_collection_fails(self):
        with self.assertRaises(RuntimeError):
            runner.check_collection([], ["TestCalendar", "TestScheduling"])
        with self.assertRaises(RuntimeError):
            runner.check_collection(["x::TestCalendar::testRead"],
                                    ["TestCalendar", "TestScheduling"])


if __name__ == "__main__":
    unittest.main()
