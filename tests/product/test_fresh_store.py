# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Failure boundaries of the fresh-store gate, against stand-in server binaries.

Run with `python3 -B -m unittest discover -s tests/product -p test_fresh_store.py -v`.
Each stand-in prints the administrator credential the harness hands it, so every test
also checks that the retained logs and the printed output never carry it.
"""
import base64
import contextlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from unittest.mock import patch

import fresh_store

SCRIPT = Path(fresh_store.__file__).resolve()

STAND_IN = textwrap.dedent('''\
    import base64, http.server, json, os, signal, sys, time, urllib.parse
    MODE, SIDE = {mode!r}, {side!r}
    admin = os.environ.get("STALWART_RECOVERY_ADMIN", "")
    with open(os.path.join(SIDE, "secret"), "w") as out:
        out.write(admin.partition(":")[2])
    with open(os.path.join(SIDE, "pids"), "a") as out:
        out.write(str(os.getpid()) + "\\n")
    print("stand-in: admin", admin, base64.b64encode(admin.encode()).decode(), flush=True)
    starts = int(open("starts").read()) + 1 if os.path.exists("starts") else 1
    with open("starts", "w") as out:
        out.write(str(starts))
    if MODE == "exit" or (MODE == "restart-crash" and starts == 3):
        sys.exit(1)
    if MODE == "hang":
        time.sleep(600)
        sys.exit(0)

    def stop(signum, frame):
        sys.exit(0)

    signal.signal(signal.SIGTERM, stop)

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, value):
            body = json.dumps(value).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            self.reply({{"primaryAccounts": {{"urn:stalwart:jmap": "a"}}}})

        def do_POST(self):
            call = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            method, arguments, tag = call["methodCalls"][0]
            if method.endswith("/get"):
                result = {{"list": []}}
            elif method.endswith("/query"):
                result = {{"ids": []}}
            elif MODE == "provision-fail":
                result = {{"notCreated": {{"new": {{"type": "invalidProperties"}}}}}}
            elif "create" in arguments:
                result = {{"created": {{"new": {{"id": "x"}}}}}}
            else:
                result = {{"updated": {{key: None for key in arguments.get("update", {{}})}}}}
            self.reply({{"methodResponses": [[method, result, tag]]}})

    port = urllib.parse.urlsplit(os.environ["STALWART_PUBLIC_URL"]).port
    http.server.ThreadingHTTPServer.allow_reuse_address = True
    http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
''')


class FreshStoreFailureTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="fresh-store-test-")).resolve()
        self.side = self.root / "side"
        self.side.mkdir()
        self.logs = self.root / "logs"

    def tearDown(self):
        for pid in self.pids():
            with contextlib.suppress(ProcessLookupError):
                os.kill(pid, 9)
        shutil.rmtree(self.root, ignore_errors=True)

    def stand_in(self, mode):
        path = self.root / f"stand-in-{mode}"
        path.write_text(f"#!{sys.executable} -I\n" + STAND_IN.format(mode=mode,
                                                                      side=str(self.side)))
        path.chmod(0o700)
        return path

    def pids(self):
        path = self.side / "pids"
        return [int(line) for line in path.read_text().split()] if path.exists() else []

    def run_cli(self, binary, *extra, timeout=120):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), "--binary", str(binary),
                               "--log-dir", str(self.logs), *extra],
                              capture_output=True, text=True, timeout=timeout,
                              cwd=self.root)

    def run_in_process(self, binary, **patches):
        output = io.StringIO()
        with contextlib.ExitStack() as stack:
            for name, value in patches.items():
                stack.enter_context(patch.object(fresh_store.Gate, name, value))
            stack.enter_context(contextlib.redirect_stdout(output))
            stack.enter_context(contextlib.redirect_stderr(output))
            code = fresh_store.Run(binary, self.logs).run()
        return code, output.getvalue()

    def assert_failed_and_sanitized(self, code, printed, reason):
        self.assertNotEqual(code, 0)
        harness_log = (self.logs / "fresh-store.log").read_text()
        self.assertIn("FAILED: ", harness_log)
        self.assertIn(reason, harness_log)
        self.assertIn(reason, printed)
        secret_file = self.side / "secret"
        if secret_file.exists():
            secret = secret_file.read_text()
            self.assertTrue(secret)
            server_log = (self.logs / "server.log").read_text()
            self.assertIn("stand-in: admin", server_log)
            self.assertIn("[REDACTED]", server_log)
            for value in (secret, base64.b64encode(("client-ci-admin:" + secret).encode())
                          .decode()):
                for text in (server_log, harness_log, printed):
                    self.assertNotIn(value, text)
        for pid in self.pids():
            with self.assertRaises(ProcessLookupError, msg="stand-in left running"):
                os.kill(pid, 0)

    def test_wrong_binary_path(self):
        result = self.run_cli(self.root / "no-such-binary")
        self.assert_failed_and_sanitized(result.returncode, result.stdout + result.stderr,
                                         "does not exist or is not executable")
        self.assertEqual((self.logs / "server.log").read_text(), "")

    def test_failed_startup(self):
        result = self.run_cli(self.stand_in("exit"))
        self.assert_failed_and_sanitized(result.returncode, result.stdout + result.stderr,
                                         "exited before readiness")

    def test_failed_provisioning(self):
        result = self.run_cli(self.stand_in("provision-fail"))
        self.assert_failed_and_sanitized(result.returncode, result.stdout + result.stderr,
                                         "x:NetworkListener/set: notCreated")

    def test_restart_crash(self):
        code, printed = self.run_in_process(
            self.stand_in("restart-crash"), create=lambda gate: None,
            snapshot_pins=lambda gate: {}, snapshot_shares=lambda gate: {},
            wait_for_drain=lambda gate: None)
        self.assert_failed_and_sanitized(code, printed, "stop cleanly and restart")
        self.assertIn("exited before readiness", printed)
        self.assertEqual(len(self.pids()), 3)

    def test_missing_readback(self):
        def partial(gate, pins, shares):
            for check in fresh_store.CHECKS[:-1]:
                gate.passed_check(check)

        code, printed = self.run_in_process(
            self.stand_in("serve"), create=lambda gate: None,
            snapshot_pins=lambda gate: {}, snapshot_shares=lambda gate: {},
            wait_for_drain=lambda gate: None, readback=partial)
        self.assert_failed_and_sanitized(code, printed,
                                         "readback did not report: " + fresh_store.CHECKS[-1])

    def test_missing_message_fails_its_check(self):
        code, printed = self.run_in_process(
            self.stand_in("serve"), create=self.fake_seed,
            snapshot_pins=lambda gate: {}, snapshot_shares=lambda gate: {},
            wait_for_drain=lambda gate: None, fetch_all=lambda gate, name: [])
        self.assert_failed_and_sanitized(code, printed,
                                         "ordinary: the IMAP APPEND message is missing")

    @staticmethod
    def fake_seed(gate):
        for name in (fresh_store.ORDINARY, fresh_store.ALPHA, fresh_store.BETA):
            gate.expected_mail[name] = {"append": ("subject", b"payload", b"raw"),
                                        "smtp": ("subject", b"payload")}

    def test_timeout(self):
        started = time.monotonic()
        result = self.run_cli(self.stand_in("hang"), "--timeout", "3")
        self.assertLess(time.monotonic() - started, 30)
        self.assert_failed_and_sanitized(result.returncode, result.stdout + result.stderr,
                                         "timed out after 3 seconds")

    def test_existing_state_directory_is_refused(self):
        existing = self.root / "existing-store"
        existing.mkdir()
        (existing / "data").mkdir()
        result = self.run_cli(self.stand_in("serve"), "--state-dir", str(existing))
        self.assert_failed_and_sanitized(result.returncode, result.stdout + result.stderr,
                                         "state directory already exists")
        self.assertEqual(self.pids(), [])
        self.assertEqual(sorted(path.name for path in existing.iterdir()), ["data"])

    def test_existing_log_directory_is_refused(self):
        self.logs.mkdir()
        result = self.run_cli(self.stand_in("serve"))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.pids(), [])
        self.assertEqual(list(self.logs.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
