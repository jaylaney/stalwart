# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Offline regression tests for the CI harness's failure boundaries."""
import base64
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
from unittest.mock import patch
import sys
import unittest

import pytest
import runner


class RunnerTests(unittest.TestCase):
    def test_collection_requires_exact_mode_lists_and_unique_nonempty_ids(self):
        existing = json.loads((runner.HERE / "collection.json").read_text())["plain"]
        self.assertEqual(len(existing), 72)
        names = [
            "flows/test_shared_calendar_view_and_edit",
            "flows/test_shared_calendar_properties_and_etags",
            "flows/test_shared_calendar_revoke_regrant_rename_and_sync",
            "flows/test_shared_calendar_delegated_invitation_update_cancel",
            "flows/test_shared_calendar_own_and_shared_replies_and_sole_decline",
        ]
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "collection.json"
            valid = {"plain": existing, "key": existing + names}
            path.write_text(json.dumps(valid))
            self.assertEqual(runner.read_collection("plain", path), set(existing))
            self.assertEqual(runner.read_collection("key", path), set(existing + names))
            for bad in ([], {"plain": existing}, {"plain": [], "key": names},
                        {"plain": existing, "key": existing + names, "other": []},
                        {"plain": existing + [existing[0]], "key": existing + names},
                        {"plain": existing, "key": existing + [""]},
                        {"plain": existing, "key": existing + [3]}):
                with self.subTest(bad=type(bad).__name__):
                    path.write_text(json.dumps(bad))
                    with self.assertRaises(ValueError):
                        runner.read_collection("plain", path)
            path.write_text(json.dumps(valid))
            with self.assertRaises(ValueError):
                runner.read_collection("missing", path)

    def test_checked_in_collection_requires_72_plain_and_77_key_cases(self):
        plain = runner.read_collection("plain")
        key = runner.read_collection("key")
        self.assertEqual(len(plain), 72)
        self.assertEqual(len(key), 77)
        self.assertEqual(key - plain, {
            "flows/test_shared_calendar_view_and_edit",
            "flows/test_shared_calendar_properties_and_etags",
            "flows/test_shared_calendar_revoke_regrant_rename_and_sync",
            "flows/test_shared_calendar_delegated_invitation_update_cancel",
            "flows/test_shared_calendar_own_and_shared_replies_and_sole_decline",
        })

    def test_runner_selects_sharing_file_only_for_key_mode(self):
        users = [{"url": "http://127.0.0.1/dav/cal/alice/", "username": "alice",
                  "password": "fixture"}] * 3
        selections = []

        def fake_run(command, **kwargs):
            selections.append((command, kwargs["env"]["CALDAV_CI_MODE"]))
            return subprocess.CompletedProcess(command, 0)

        with tempfile.TemporaryDirectory() as raw, \
                patch.object(runner, "materialize_source") as materialize, \
                patch.object(runner.subprocess, "run", fake_run):
            def create_suite(source, destination):
                (destination / "tests").mkdir(parents=True)
            materialize.side_effect = create_suite
            for mode in ("plain", "key"):
                work = Path(raw) / mode
                work.mkdir()
                runner.run_suite(Path(raw), users, mode, work)
        self.assertEqual(len(selections), 2)
        for command, mode in selections:
            self.assertIn(str(runner.HERE / "test_flows.py"), command)
            self.assertEqual(str(runner.HERE / "test_sharing.py") in command, mode == "key")

    def test_public_sharing_api_error_does_not_disclose_response_or_credentials(self):
        import test_sharing
        user = {"url": "http://127.0.0.1:1234/dav/cal/alice/",
                "username": "alice", "password": "sentinel-password"}
        failure = runner.urllib.error.HTTPError(
            "http://127.0.0.1:1234/api/vault/shares/grant", 409, "conflict", {},
            io.BytesIO(b'{"error":"sentinel-response"}'))
        with patch.object(test_sharing.request, "build_opener") as opener:
            opener.return_value.open.side_effect = failure
            with self.assertRaisesRegex(AssertionError, "HTTP 409") as raised:
                test_sharing.api(user, "shares/grant", calendar="fixture", address="bob",
                                 level="view")
        self.assertNotIn("sentinel-password", str(raised.exception))
        self.assertNotIn("sentinel-response", str(raised.exception))

    def test_public_sharing_api_returns_only_selected_response_fields(self):
        import test_sharing
        user = {"url": "http://127.0.0.1:1234/dav/cal/alice/",
                "username": "alice", "password": "fixture"}
        response = io.BytesIO(b'{"fingerprint":"display-value","other":"secret"}')
        response.status = 200
        with patch.object(test_sharing.request, "build_opener") as opener:
            opener.return_value.open.return_value = response
            result = test_sharing.api(user, "contacts/lookup", address="bob")
        self.assertEqual(result, {"fingerprint": "display-value"})

    def test_public_sharing_api_failures_do_not_expose_decoded_sentinels_under_pytest(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            test = root / "test_api_failures.py"
            test.write_text('''
import io
import json
import os
from types import SimpleNamespace
import pytest
import test_sharing

USER = {"url": "http://127.0.0.1:1234/dav/cal/alice/",
        "username": "alice", "password": "fixture"}
RECIPIENT = {"username": "bob"}

@pytest.mark.parametrize("case", ["nonobject", "pin", "grant", "revoke"])
def test_safe_failure(case, monkeypatch):
    secret = os.environ["API_RESPONSE_SENTINEL"]
    if case == "nonobject":
        reply = io.BytesIO(json.dumps([secret]).encode())
        reply.status = 200
        opener = SimpleNamespace(open=lambda *args, **kwargs: reply)
        monkeypatch.setattr(test_sharing.request, "build_opener", lambda *args: opener)
        test_sharing.api(USER, "contacts/lookup", address="bob")
    else:
        payloads = {"pin": {"fingerprint": [secret]},
                    "grant": {"ok": [secret], "changed": True},
                    "revoke": {"ok": True, "changed": [secret]}}
        monkeypatch.setattr(test_sharing, "api", lambda *args, **kwargs: payloads[case])
        if case == "pin":
            test_sharing.pin(USER, RECIPIENT)
        elif case == "grant":
            test_sharing.grant(USER, "shared", RECIPIENT, "view")
        else:
            test_sharing.revoke(USER, "shared", RECIPIENT)
''')
            secret = "runtime-response-sentinel"
            env = {**os.environ, "PYTHONPATH": str(runner.HERE),
                   "PYTEST_DISABLE_PLUGIN_AUTOLOAD": "1", "PYTEST_ADDOPTS": "",
                   "API_RESPONSE_SENTINEL": secret}
            result = subprocess.run([sys.executable, "-m", "pytest", "-q", str(test)],
                                    cwd=root, env=env, text=True, capture_output=True)
            output = result.stdout + result.stderr
            self.assertEqual(result.returncode, 1, output)
            self.assertIn("4 failed", output)
            self.assertNotIn(secret, output)

    def test_owned_poll_rejects_matching_event_observed_after_deadline(self):
        import test_sharing
        now = [0.0]
        client = SimpleNamespace(timeout=None)
        event = SimpleNamespace(url="http://fixture/dav/cal/bob/default/x.ics",
                                load=lambda: None)

        def late_event(uid):
            now[0] = 16.0
            return event

        calendar = SimpleNamespace(url="http://fixture/dav/cal/bob/default/",
                                   event_by_uid=late_event)
        principal = SimpleNamespace(client=client, calendars=lambda: [calendar])
        with patch.object(test_sharing.time, "monotonic", side_effect=lambda: now[0]), \
                self.assertRaises(pytest.fail.Exception):
            test_sharing.poll_owned(principal, "bob", "x")
        self.assertIsNone(client.timeout)

    def test_source_poll_rejects_late_match_and_restores_transport_timeout(self):
        import test_sharing
        now = [0.0]
        client = SimpleNamespace(timeout=27)

        def late_read():
            self.assertEqual(client.timeout, 15)
            now[0] = 16.0
            return "accepted"

        with patch.object(test_sharing.time, "monotonic", side_effect=lambda: now[0]), \
                self.assertRaises(pytest.fail.Exception):
            test_sharing.poll_state(client, late_read, lambda value: value == "accepted",
                                    "source state")
        self.assertEqual(client.timeout, 27)

    def test_source_poll_timeout_is_fatal_and_restores_transport_timeout(self):
        import test_sharing
        client = SimpleNamespace(timeout=None)
        seen = []

        def timed_out():
            seen.append(client.timeout)
            raise TimeoutError("fixture timeout")

        with self.assertRaises(TimeoutError):
            test_sharing.poll_state(client, timed_out, lambda value: True, "source state")
        self.assertEqual(len(seen), 1)
        self.assertGreater(seen[0], 0)
        self.assertLessEqual(seen[0], 15)
        self.assertIsNone(client.timeout)

    def test_actor_discovery_matches_encoded_owner_to_literal_at_href(self):
        import test_sharing
        from types import SimpleNamespace
        owner = "http://127.0.0.1/dav/cal/alice%40example.test/shared/"
        discovered = SimpleNamespace(url="http://127.0.0.1/dav/cal/alice@example.test/shared/")
        private = SimpleNamespace(url="http://127.0.0.1/dav/cal/alice@example.test/private/")
        principal = SimpleNamespace(calendars=lambda: [discovered, private])
        self.assertIs(test_sharing.actor_calendar(principal, owner), discovered)

    def test_dav_status_helper_distinguishes_401_from_403(self):
        import test_sharing
        from types import SimpleNamespace
        for status in (401, 403):
            with self.subTest(status=status):
                response = SimpleNamespace(status_code=status)
                session = SimpleNamespace(request=lambda *args, **kwargs: response)
                client = SimpleNamespace(auth=object(), session=session, timeout=10,
                                         ssl_verify_cert=True, ssl_cert=None,
                                         _prepare_request=lambda *args: ("http://fixture", {}))
                with patch.object(test_sharing, "DAVResponse", return_value=response):
                    observed, _ = test_sharing.dav(client, "http://fixture")
                self.assertEqual(observed, status)

    def test_jmap_partial_error_is_fatal_and_does_not_disclose_response(self):
        response = {"methodResponses": [["x:Account/set", {
            "created": {"a": {"id": "1"}},
            "notCreated": {"b": {"description": "secret-password"}},
        }, "0"]]}
        with self.assertRaisesRegex(RuntimeError, "notCreated") as failure:
            runner.checked_jmap(response, "x:Account/set")
        self.assertNotIn("secret-password", str(failure.exception))

    def test_malformed_api_json_is_an_operational_error_without_response_body(self):
        with tempfile.TemporaryDirectory() as raw:
            with patch.object(runner, "port", return_value=12345):
                server = runner.ProductServer("unused", Path(raw))
            try:
                with patch.object(runner.urllib.request, "build_opener") as opener:
                    opener.return_value.open.return_value = io.BytesIO(
                        b'{"recovery_key": "sentinel-secret", invalid-json}')
                    with self.assertRaisesRegex(RuntimeError, "invalid JSON") as failure:
                        server.request("/api/vault/setup", auth=False)
                self.assertNotIn("sentinel-secret", str(failure.exception))
                self.assertNotIn("recovery_key", str(failure.exception))
            finally:
                server.close()

    def test_malformed_api_shapes_fail_safely_and_still_run_the_second_mode(self):
        cases = {
            "object": ["secret-response-sentinel"],
            "session": {"primaryAccounts": []},
            "jmap": {"methodResponses": [[]]},
            "jmap-result": {"methodResponses": [["x:Thing/get", [], "0"]]},
            "create": {"methodResponses": [["x:Thing/set", {
                "created": {"new": {"id": ["secret-response-sentinel"]}}}, "0"]]},
            "objects": {"methodResponses": [["x:Thing/get", {
                "list": [{"secret-response-sentinel": True}]}, "0"]]},
            "update": {"methodResponses": [["x:Thing/set", {"updated": 42}, "0"]]},
            "token": {"token": ["secret-response-sentinel"]},
            "setup": {"recovery_key": {"secret-response-sentinel": True}},
        }
        original = runner.ProductServer
        spawn = subprocess.Popen
        for case, payload in cases.items():
            with self.subTest(case=case), tempfile.TemporaryDirectory() as raw:
                instances = []
                suites = []

                class FixtureServer(original):
                    def __init__(self, binary, directory):
                        super().__init__(binary, directory)
                        self.account_id = "fixture-id"
                        self.bootstrap_completed = False
                        instances.append(self)

                    def bootstrap(self):
                        if case != "session" or len(instances) != 1:
                            self.child = spawn([sys.executable, "-c",
                                                "import time; time.sleep(60)"])
                        if len(instances) != 1:
                            return
                        if case == "object":
                            self.request("/api/vault/setup", auth=False)
                        elif case == "session":
                            self.start(recovery=True)
                        elif case in ("jmap", "jmap-result"):
                            self.jmap("x:Thing/get", {})
                        elif case == "create":
                            self.create("Thing", {})
                        elif case == "objects":
                            self.objects("Thing")
                        elif case == "update":
                            self.update("Thing", "fixture-id", {})
                        else:
                            self.users("key")
                        self.bootstrap_completed = True

                    def users(self, mode):
                        return [] if len(instances) > 1 else super().users(mode)

                    def close(self):
                        self.reaped = self.child
                        super().close()

                def response(request, timeout):
                    value = payload
                    if case in ("token", "setup"):
                        if request.full_url.endswith("/jmap/"):
                            method = json.loads(request.data)["methodCalls"][0][0]
                            value = {"methodResponses": [[method, {
                                "created": {"new": {"id": "fixture-id"}}}, "0"]]}
                        elif request.full_url.endswith("setup-token") and case == "setup":
                            value = {"token": "setup-secret"}
                    return io.BytesIO(json.dumps(value).encode())

                def launch(command, **kwargs):
                    return spawn([sys.executable, "-c", "import time; time.sleep(60)"], **kwargs)

                output = io.StringIO()
                with patch.object(runner, "ProductServer", FixtureServer), \
                        patch.object(runner, "port", return_value=12345), \
                        patch.object(runner.subprocess, "Popen", launch), \
                        patch.object(runner.urllib.request, "build_opener") as opener, \
                        patch.object(runner, "run_suite", side_effect=lambda *args: \
                                     suites.append(args[2])), \
                        patch.object(runner.time, "monotonic", side_effect=[0, 0, 61]), \
                        patch.object(runner.time, "sleep"), \
                        contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                    opener.return_value.open.side_effect = response
                    result = runner.run_modes("unused", "unused", ("plain", "key"))
                self.assertEqual(result, 1)
                self.assertEqual(suites, ["key"])
                self.assertFalse(instances[0].bootstrap_completed)
                self.assertEqual(len(instances), 2)
                for server in instances:
                    self.assertIsNotNone(server.reaped.returncode)
                self.assertNotIn("secret-response-sentinel", output.getvalue())
                self.assertIn("(plain): FAILED", output.getvalue())
                self.assertIn("(key): passed", output.getvalue())
                self.assertTrue(all(not server.directory.exists() for server in instances))

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
                root = Path(raw).resolve()
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
                (root / "collection.json").write_text(json.dumps({
                    "plain": ["calendar/testKnown"], "key": ["calendar/testKnown"]}))
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
            root = Path(raw).resolve()
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
            root = Path(raw).resolve()
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

    def policy_case(self, root, source, outcomes=None, features=None, expected=None,
                    mode="plain", filename="test_sample.py"):
        test = root / filename
        test.write_text(source)
        (root / "policy.json").write_text(json.dumps({mode: {
            "features": features or {}, "outcomes": outcomes or {}}}))
        cases = expected if expected is not None else ["calendar/testKnown"]
        (root / "collection.json").write_text(json.dumps({
            "plain": cases if mode == "plain" else ["calendar/testKnown"],
            "key": cases if mode == "key" else ["calendar/testKnown"]}))
        (root / "conftest.py").write_text(
            "from pathlib import Path\nimport policy_plugin\n"
            "policy_plugin.HERE = Path(__file__).parent\n")
        env = {**os.environ, "PYTHONPATH": str(runner.HERE), "CALDAV_CI_MODE": mode,
               "CALDAV_CI_POLICY": str(root / "policy.json"),
               "PYTEST_DISABLE_PLUGIN_AUTOLOAD": "1", "PYTEST_ADDOPTS": ""}
        return subprocess.run([sys.executable, "-m", "pytest", "-p", "policy_plugin", "-q",
                               str(test)], env=env, cwd=root, text=True, capture_output=True)

    def test_reasonless_upstream_xfail_and_xpass_cannot_match_skip_policy(self):
        for body in ("assert False", "pass"):
            with self.subTest(body=body), tempfile.TemporaryDirectory() as raw:
                source = ("import pytest\nclass TestForServerStalwartPlain:\n"
                          "    @pytest.mark.xfail(reason='')\n"
                          "    def testKnown(self):\n        " + body + "\n")
                result = self.policy_case(Path(raw), source, {
                    "calendar/testKnown": {"outcome": "skipped", "reason": "fixture",
                                           "source": "fixture"}})
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("COMPATIBILITY POLICY", result.stdout)

    def test_malformed_policy_is_usage_error_before_test_runs(self):
        records = [
            ({"calendar/testKnown": []}, {}),
            ({"calendar/testUnknown": {"outcome": "skipped", "reason": "why",
                                       "source": "fixture"}}, {}),
            ({"calendar/testKnown": {"outcome": "xfailed", "reason": "why",
                                     "source": "fixture", "exception": "BogusError",
                                     "failure": {"file": "test_sample.py", "line": 3}}}, {}),
            ({}, {"not-a-real-feature": {"value": True, "reason": "why", "source": "fixture"}}),
            ({}, {"scheduling.auto-schedule": []}),
            ({}, {"scheduling.auto-schedule": {"value": 42, "reason": "why",
                                             "source": "fixture"}}),
        ]
        for outcomes, features in records:
            with self.subTest(outcomes=outcomes, features=features), \
                    tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                source = ("class TestForServerStalwartPlain:\n    def testKnown(self):\n"
                          "        open('ran', 'w').close()\n")
                result = self.policy_case(root, source, outcomes, features)
                output = result.stdout + result.stderr
                self.assertEqual(result.returncode, 4, output)
                self.assertNotIn("INTERNALERROR", output)
                self.assertFalse((root / "ran").exists())

    def test_manifest_rejects_missing_and_extra_items_before_execution(self):
        for expected in ([], ["calendar/testKnown", "calendar/testMissing"]):
            with self.subTest(expected=expected), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                result = self.policy_case(root, "class TestForServerStalwartPlain:\n"
                                         "    def testKnown(self):\n"
                                         "        open('ran', 'w').close()\n", expected=expected)
                self.assertEqual(result.returncode, 4, result.stdout + result.stderr)
                self.assertFalse((root / "ran").exists())

    def test_key_sharing_manifest_rejects_missing_and_extra_cases_before_execution(self):
        name = "flows/test_shared_calendar_view_and_edit"
        source = ("def test_shared_calendar_view_and_edit():\n"
                  "    open('ran', 'w').close()\n")
        for cases in (["calendar/testKnown"], [name, "flows/test_unexpected_sharing"]):
            with self.subTest(cases=cases), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                result = self.policy_case(root, source, expected=cases, mode="key",
                                          filename="test_sharing.py")
                self.assertEqual(result.returncode, 4, result.stdout + result.stderr)
                self.assertFalse((root / "ran").exists())

    def test_each_mode_reaps_and_removes_fixtures_despite_failures(self):
        for stage in ("bootstrap", "users", "suite", "close", "export"):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as raw:
                root = Path(raw).resolve()
                instances = []
                secret = "sentinel-password"
                basic = base64.b64encode(("alice:" + secret).encode()).decode()

                class FakeServer:
                    def __init__(self, binary, directory):
                        self.directory = directory
                        self.redactor = runner.Redactor()
                        self.redactor.credentials("alice", secret)
                        self.log = open(directory / "server.log", "wb")
                        self.log.write((secret + " Basic " + basic + "\n").encode())
                        self.child = subprocess.Popen([
                            sys.executable, "-c", "import time; time.sleep(60)"])
                        (directory / "data").mkdir()
                        (directory / "data" / "private").write_text(secret)
                        (directory / "store.json").write_text(secret)
                        instances.append(self)

                    def fail(self, where):
                        if len(instances) == 1 and stage == where:
                            raise RuntimeError(where + ": " + secret)

                    def bootstrap(self):
                        self.fail("bootstrap")

                    def users(self, mode):
                        self.fail("users")
                        return []

                    def close(self):
                        runner.stop_child(self.child)
                        self.log.close()
                        self.fail("close")

                def suite(source, users, mode, work):
                    (work / "users.json").write_text(secret)
                    (work / "pytest.log").write_text(secret + " Basic " + basic)
                    instances[-1].fail("suite")

                original_retain = runner.retain_log

                def retain(directory, name, text):
                    if stage == "export" and name == "plain-server.log":
                        raise OSError("export: " + secret)
                    original_retain(directory, name, text)

                logs = root / "diagnostics"
                output = io.StringIO()
                with patch.object(runner, "ProductServer", FakeServer), \
                        patch.object(runner, "run_suite", suite), \
                        patch.object(runner, "retain_log", retain), \
                        contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                    result = runner.run_modes("unused", "unused", ("plain", "key"), logs)
                self.assertEqual(result, 1)
                self.assertEqual(len(instances), 2)
                for server in instances:
                    self.assertIsNotNone(server.child.returncode)
                    self.assertFalse(server.directory.exists())
                expected = {"plain-server.log", "plain-pytest.log",
                            "key-server.log", "key-pytest.log"}
                if stage == "export":
                    expected.remove("plain-server.log")
                self.assertEqual({p.name for p in logs.iterdir()}, expected)
                exported = output.getvalue() + "".join(p.read_text() for p in logs.iterdir())
                self.assertNotIn(secret, exported)
                self.assertNotIn(basic, exported)
                self.assertIn("[REDACTED]", exported)
                self.assertEqual(logs.stat().st_mode & 0o777, 0o700)
                self.assertTrue(all(p.stat().st_mode & 0o777 == 0o600 for p in logs.iterdir()))

    def test_log_export_cannot_follow_replaced_output_directory(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw).resolve()
            output = root / "logs"
            directory = runner.log_directory(output)
            original = root / "original"
            output.rename(original)
            elsewhere = root / "elsewhere"
            elsewhere.mkdir()
            output.symlink_to(elsewhere, target_is_directory=True)
            try:
                runner.retain_log(directory, "plain-server.log", "safe")
                self.assertEqual(list(elsewhere.iterdir()), [])
                self.assertEqual((original / "plain-server.log").read_text(), "safe")
            finally:
                if isinstance(directory, int):
                    os.close(directory)

    def test_vault_provisioning_collects_admin_old_new_passwords_and_material(self):
        with tempfile.TemporaryDirectory() as raw:
            with patch.object(runner, "port", return_value=12345):
                server = runner.ProductServer("unused", Path(raw))
            observed = [server.admin]
            material = []

            def request(path, body=None, auth=True):
                if path.endswith("setup-token"):
                    value = "setup-token-" + str(len(material))
                    material.append(value)
                    return {"token": value}
                if path.endswith("/setup"):
                    observed.append((body["username"], body["password"]))
                    value = "recovery-key-" + str(len(material))
                    material.append(value)
                    return {"recovery_key": value}
                observed.append((body["username"], body["new_password"]))
                return {}

            try:
                with patch.object(server, "create", return_value="fixture-id"), \
                        patch.object(server, "request", request):
                    server.users("key")
                self.assertEqual(len(observed), 7)
                self.assertEqual(len(material), 6)
                for username, password in observed:
                    basic = base64.b64encode((username + ":" + password).encode()).decode()
                    self.assertNotIn(password, server.redactor.sanitize(password))
                    self.assertNotIn(basic, server.redactor.sanitize("Basic " + basic))
                for value in material:
                    self.assertNotIn(value, server.redactor.sanitize(value))
            finally:
                server.close()

    def test_retained_log_directory_rejects_existing_or_symlink_paths(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw).resolve()
            real = root / "real"
            real.mkdir()
            link = root / "link"
            link.symlink_to(real, target_is_directory=True)
            for output in (real, link / "new", root / ".." / "unsafe"):
                with self.subTest(output=output), self.assertRaises((OSError, ValueError)):
                    runner.run_modes("unused", "unused", (), output)
            self.assertEqual(list(real.iterdir()), [])



if __name__ == "__main__":
    unittest.main()
