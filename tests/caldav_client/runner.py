# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Run pinned python-caldav suites against a disposable product server."""
from __future__ import annotations

import argparse
import base64
import io
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import tarfile
import time
import urllib.error
import urllib.request

PIN = "111ae14b875f40d100d5259820f10210f00e20f9"
HERE = Path(__file__).resolve().parent


def checked_jmap(response, method):
    replies = response.get("methodResponses", [])
    if len(replies) != 1 or replies[0][0] != method or replies[0][2] != "0":
        raise RuntimeError(f"JMAP {method} failed or returned an unexpected method")
    result = replies[0][1]
    for field in ("notCreated", "notUpdated", "notDestroyed"):
        if result.get(field):
            raise RuntimeError(f"JMAP {method}: {field}")
    return result


def stop_child(child, timeout=10):
    if child is None:
        return
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            child.kill()
    child.wait(timeout=5)


def check_outcomes(outcomes, policy):
    for name, record in policy.items():
        if name.startswith("flows/"):
            raise ValueError("Strict client flows cannot have compatibility exceptions")
        if record.get("outcome") not in ("skipped", "xfailed") or not record.get("reason"):
            raise ValueError(f"Invalid compatibility outcome record: {name}")
    problems = []
    for name, outcome in outcomes.items():
        expected = policy.get(name, {}).get("outcome", "passed")
        if outcome != expected:
            problems.append(f"{name}: expected {expected}, got {outcome}")
    for name in policy.keys() - outcomes.keys():
        problems.append(f"policy test was not collected: {name}")
    return problems


def check_collection(nodeids, classes, expected=None):
    if not nodeids or any(not any(f"::{name}::" in n for n in nodeids) for name in classes):
        raise RuntimeError("Expected nonempty calendar and scheduling classes were not collected")
    if expected is not None and set(nodeids) != set(expected):
        raise RuntimeError("Pinned test collection is incomplete or unexpected")


def materialize_source(source, destination):
    archive = subprocess.check_output(["git", "-C", str(source), "archive", PIN])
    destination.mkdir(mode=0o700)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tree:
        tree.extractall(destination, filter="data")


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def private_json(path, value):
    with open(path, "x", encoding="utf-8") as output:
        os.chmod(path, 0o600)
        json.dump(value, output)


class ProductServer:
    def __init__(self, binary, directory):
        self.binary = str(Path(binary).resolve())
        self.directory = directory
        self.child = None
        self.admin = ("client-ci-admin", secrets.token_urlsafe(32))
        self.http_port = port()
        self.url = f"http://127.0.0.1:{self.http_port}"
        self.config = directory / "store.json"
        private_json(self.config, {"@type": "RocksDb", "path": str(directory / "data")})
        self.log = open(directory / "server.log", "wb")
        os.chmod(directory / "server.log", 0o600)

    def request(self, path, body=None, auth=True):
        headers = {"Content-Type": "application/json"}
        if auth:
            token = base64.b64encode(":".join(self.admin).encode()).decode()
            headers["Authorization"] = "Basic " + token
        request = urllib.request.Request(self.url + path, headers=headers,
                                         data=None if body is None else json.dumps(body).encode())
        try:
            with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(
                    request, timeout=10) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            # Do not echo response bodies: provisioning responses may include secrets.
            raise RuntimeError(f"public API {path} returned HTTP {error.code}") from None

    def jmap(self, method, arguments):
        result = self.request("/jmap/", {
            "using": ["urn:ietf:params:jmap:core", "urn:stalwart:jmap"],
            "methodCalls": [[method, {"accountId": self.account_id, **arguments}, "0"]],
        })
        return checked_jmap(result, method)

    def create(self, kind, value):
        result = self.jmap(f"x:{kind}/set", {"create": {"new": value}})
        if set(result.get("created", {})) != {"new"}:
            raise RuntimeError(f"JMAP x:{kind}/set did not create object")
        return result["created"]["new"]["id"]

    def objects(self, kind):
        return self.jmap(f"x:{kind}/get", {})["list"]

    def update(self, kind, object_id, values):
        result = self.jmap(f"x:{kind}/set", {"update": {object_id: values}})
        if object_id not in result.get("updated", {}):
            raise RuntimeError(f"JMAP x:{kind}/set did not update object")

    def start(self, recovery):
        env = os.environ.copy()
        # Never let the caller's runtime/service configuration affect this fixture.
        for key in list(env):
            if key.startswith(("STALWART_", "ZA_")) or key == "CONFIG_PATH":
                del env[key]
        env.update(STALWART_RECOVERY_ADMIN=":".join(self.admin), STALWART_HOSTNAME="localhost",
                   STALWART_PUBLIC_URL=self.url)
        if recovery:
            env.update(STALWART_RECOVERY_MODE="1", STALWART_RECOVERY_MODE_PORT=str(self.http_port))
        self.child = subprocess.Popen([self.binary, "--config", str(self.config)], env=env,
                                      cwd=self.directory, stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if self.child.poll() is not None:
                raise RuntimeError("Disposable server exited before readiness")
            try:
                session = self.request("/jmap/session")
                self.account_id = session["primaryAccounts"]["urn:stalwart:jmap"]
                return
            except (RuntimeError, OSError):
                time.sleep(0.2)
        raise RuntimeError("Disposable server did not become ready within 60 seconds")

    def bootstrap(self):
        self.start(recovery=True)
        for application in self.objects("Application"):
            self.update("Application", application["id"], {"enabled": False})
        self.create("NetworkListener", {"name": "client-ci", "protocol": "http",
                    "bind": {f"127.0.0.1:{self.http_port}": True}, "useTls": False})
        self.create("Asn", {"@type": "Disabled"})
        existing = self.objects("SpamSettings")
        if existing:
            for obj in existing:
                self.update("SpamSettings", obj["id"],
                            {"enable": False, "spamFilterRulesUrl": None})
        else:
            self.create("SpamSettings", {"enable": False, "spamFilterRulesUrl": None})
        self.create("Tracer", {"@type": "Stdout", "enable": True, "ansi": False,
                               "level": "warn"})
        stop_child(self.child)
        self.child = None
        self.start(recovery=False)

    def users(self, mode):
        domain_name = f"{mode}.client-ci.test"
        domain = self.create("Domain", {"name": domain_name, "isEnabled": True,
                            "certificateManagement": {"@type": "Manual"},
                            "dnsManagement": {"@type": "Manual"},
                            "dkimManagement": {"@type": "Manual"}})
        users = []
        for name in ("alice", "bob", "carol"):
            username = f"{name}@{domain_name}"
            password = secrets.token_urlsafe(32)
            credentials = {} if mode == "key" else {"0": {"@type": "Password", "secret": password}}
            self.create("Account", {"@type": "User", "name": name, "domainId": domain,
                                    "credentials": credentials})
            if mode == "key":
                token = self.request("/api/vault/setup-token", {"account": username})["token"]
                setup = self.request("/api/vault/setup", {"username": username, "token": token,
                                                         "password": password}, auth=False)
                if not setup.get("recovery_key"):
                    raise RuntimeError("Vault setup omitted recovery key")
                new_password = secrets.token_urlsafe(32)
                self.request("/api/vault/password", {"username": username, "password": password,
                                                     "new_password": new_password}, auth=False)
                password = new_password
            users.append({"url": self.url + "/dav/cal/" + username + "/",
                          "username": username, "password": password})
        return users

    def close(self):
        stop_child(self.child)
        self.child = None
        self.log.close()


def run_suite(source, users, mode, work):
    policy = json.loads((HERE / "compatibility.json").read_text())[mode]
    features = {}
    for feature, record in policy["features"].items():
        if not record.get("reason") or not record.get("source") or "value" not in record:
            raise RuntimeError(f"Undocumented compatibility feature: {feature}")
        features[feature] = record["value"]
    name = "Stalwart" + mode.capitalize()
    config = {"test-servers": {name: {"type": "external", "enabled": True, **users[0],
                                     "features": features, "scheduling_users": [
                                         {**user, "features": features} for user in users]}}}
    # Work from a private copy so even an interrupted run leaves no credentials in the source clone.
    suite = work / "upstream"
    materialize_source(source, suite)
    private_json(suite / "tests" / "caldav_test_servers.json", config)
    env = os.environ.copy()
    for key in ("PYTEST_ADDOPTS", "PYTEST_PLUGINS", "HTTP_PROXY", "HTTPS_PROXY",
                "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
        env.pop(key, None)
    env.update(NO_PROXY="127.0.0.1,localhost", PYTEST_DISABLE_PLUGIN_AUTOLOAD="1",
               PYTHON_CALDAV_TEST_EMBEDDED="0", PYTHON_CALDAV_TEST_DOCKER="0",
               PYTHON_CALDAV_TEST_EXTERNAL="1", CALDAV_CI_MODE=mode,
               CALDAV_CI_POLICY=str(HERE / "compatibility.json"),
               CALDAV_CI_USERS=str(work / "users.json"))
    private_json(work / "users.json", users)
    classes = ["TestForServer" + name, "TestSchedulingForServer" + name]
    selection = [f"tests/test_caldav.py::{cls}" for cls in classes]
    command = [sys.executable, "-m", "pytest", "-p", "policy_plugin",
               "-p", "pytest_asyncio.plugin", "--tb=short", "-q"]
    env["PYTHONPATH"] = os.pathsep.join([str(HERE), str(suite)])
    result = subprocess.run(command + selection + [str(HERE / "test_flows.py")], cwd=suite, env=env)
    if result.returncode:
        raise RuntimeError(f"{mode}: third-party client tests failed (exit {result.returncode})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--caldav-source", required=True, type=Path)
    parser.add_argument("--mode", choices=("plain", "key", "both"), default="both")
    args = parser.parse_args()
    head = subprocess.check_output(["git", "-C", str(args.caldav_source), "rev-parse", "HEAD"],
                                   text=True).strip()
    if head != PIN:
        parser.error(f"python-caldav must be checked out at {PIN}")
    with tempfile.TemporaryDirectory(prefix="stalwart-caldav-") as raw:
        work = Path(raw)
        os.chmod(work, 0o700)
        server = ProductServer(args.binary, work)
        try:
            server.bootstrap()
            for mode in ("plain", "key") if args.mode == "both" else (args.mode,):
                mode_work = work / mode
                mode_work.mkdir(mode=0o700)
                run_suite(args.caldav_source, server.users(mode), mode, mode_work)
        finally:
            server.close()


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError) as error:
        print(f"CalDAV client CI: {error}", file=sys.stderr)
        sys.exit(1)
