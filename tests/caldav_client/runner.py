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
import urllib.parse

PIN = "111ae14b875f40d100d5259820f10210f00e20f9"
HERE = Path(__file__).resolve().parent


def response_string(response, field, context):
    value = response.get(field) if isinstance(response, dict) else None
    if not isinstance(value, str) or not value:
        raise RuntimeError(f"{context} returned an invalid {field} field")
    return value


def checked_jmap(response, method):
    replies = response.get("methodResponses") if isinstance(response, dict) else None
    if (not isinstance(replies, list) or len(replies) != 1
            or not isinstance(replies[0], list) or len(replies[0]) != 3
            or replies[0][0] != method or replies[0][2] != "0"
            or not isinstance(replies[0][1], dict)):
        raise RuntimeError(f"JMAP {method} failed or returned an invalid method response")
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


def validate_policy(policy, expected):
    if not isinstance(policy, dict) or not isinstance(policy.get("outcomes"), dict):
        raise ValueError("Compatibility policy must contain outcome and feature objects")
    if not isinstance(policy.get("features"), dict):
        raise ValueError("Compatibility features must be an object")
    for name, record in policy["outcomes"].items():
        if name not in expected or not isinstance(record, dict):
            raise ValueError(f"Invalid compatibility exception: {name}")
        if any(not isinstance(record.get(field), str) or not record[field].strip()
               for field in ("reason", "source")):
            raise ValueError(f"Undocumented compatibility exception: {name}")
        if record.get("outcome") == "xfailed":
            failure = record.get("failure")
            if (record.get("exception") not in ("AssertionError", "DeprecationWarning")
                    or not isinstance(failure, dict)
                    or not isinstance(failure.get("file"), str)
                    or not failure["file"] or Path(failure["file"]).is_absolute()
                    or ".." in Path(failure["file"]).parts
                    or type(failure.get("line")) is not int or failure["line"] < 1):
                raise ValueError(f"Invalid compatibility failure fingerprint: {name}")
    check_outcomes({}, policy["outcomes"])
    from caldav.compatibility_hints import FeatureSet
    features = {}
    for name, record in policy["features"].items():
        if (not isinstance(record, dict) or type(record.get("value")) is not bool
                or any(not isinstance(record.get(field), str) or not record[field].strip()
                       for field in ("reason", "source"))):
            raise ValueError(f"Undocumented compatibility feature: {name}")
        if name not in FeatureSet.FEATURES:
            raise ValueError(f"Unknown compatibility feature: {name}")
        FeatureSet.find_feature(name)
        features[name] = record["value"]
    return features


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


class Redactor:
    """Exact generated secrets and directly derived credential representations."""
    def __init__(self):
        self.values = set()

    def secret(self, value):
        if value:
            self.values.update((value, json.dumps(value)[1:-1],
                                urllib.parse.quote(value, safe=""),
                                base64.b64encode(value.encode()).decode()))

    def credentials(self, username, password):
        self.secret(password)
        self.secret(username + ":" + password)

    def sanitize(self, value):
        for secret in sorted(self.values, key=len, reverse=True):
            value = value.replace(secret, "[REDACTED]")
        return value


def log_directory(path):
    # Walk with directory descriptors: path replacements cannot redirect retained diagnostics.
    if ".." in path.parts:
        raise ValueError("Diagnostic directory cannot contain parent traversal")
    parts = path.absolute().parts
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    descriptor = os.open(parts[0], flags)
    try:
        for part in parts[1:-1]:
            child = os.open(part, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        os.mkdir(parts[-1], mode=0o700, dir_fd=descriptor)
        return os.open(parts[-1], flags, dir_fd=descriptor)
    finally:
        os.close(descriptor)


def retain_log(directory, name, text):
    descriptor = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o600, dir_fd=directory)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        output.write(text)


class ProductServer:
    def __init__(self, binary, directory):
        self.binary = str(Path(binary).resolve())
        self.directory = directory
        self.child = None
        self.redactor = Redactor()
        self.admin = ("client-ci-admin", secrets.token_urlsafe(32))
        self.redactor.credentials(*self.admin)
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
                try:
                    result = json.load(response)
                except json.JSONDecodeError:
                    raise RuntimeError(f"public API {path} returned invalid JSON") from None
                if not isinstance(result, dict):
                    raise RuntimeError(f"public API {path} returned a non-object response")
                return result
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
        created = result.get("created")
        if not isinstance(created, dict) or set(created) != {"new"}:
            raise RuntimeError(f"JMAP x:{kind}/set did not create object")
        return response_string(created["new"], "id", f"JMAP x:{kind}/set")

    def objects(self, kind):
        objects = self.jmap(f"x:{kind}/get", {}).get("list")
        if not isinstance(objects, list):
            raise RuntimeError(f"JMAP x:{kind}/get returned an invalid list")
        for obj in objects:
            response_string(obj, "id", f"JMAP x:{kind}/get")
        return objects

    def update(self, kind, object_id, values):
        result = self.jmap(f"x:{kind}/set", {"update": {object_id: values}})
        updated = result.get("updated")
        if not isinstance(updated, dict) or object_id not in updated:
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
            env.update(STALWART_RECOVERY_MODE="1",
                       STALWART_RECOVERY_MODE_PORT=str(self.http_port),
                       STALWART_RECOVERY_MODE_LOG_LEVEL="warn")
        self.child = subprocess.Popen([self.binary, "--config", str(self.config)], env=env,
                                      cwd=self.directory, stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if self.child.poll() is not None:
                raise RuntimeError("Disposable server exited before readiness")
            try:
                session = self.request("/jmap/session")
                self.account_id = response_string(session.get("primaryAccounts"),
                                                  "urn:stalwart:jmap", "JMAP session")
                return
            except (RuntimeError, OSError):
                time.sleep(0.2)
        raise RuntimeError("Disposable server did not become ready within 60 seconds")

    def bootstrap(self):
        self.start(recovery=True)
        # Disable background application downloads: these fixtures exercise only CalDAV.
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
            self.redactor.credentials(username, password)
            credentials = {} if mode == "key" else {"0": {"@type": "Password", "secret": password}}
            self.create("Account", {"@type": "User", "name": name, "domainId": domain,
                                    "credentials": credentials})
            if mode == "key":
                token = response_string(self.request("/api/vault/setup-token",
                                                     {"account": username}),
                                        "token", "public API /api/vault/setup-token")
                self.redactor.secret(token)
                setup = self.request("/api/vault/setup", {"username": username, "token": token,
                                                         "password": password}, auth=False)
                recovery_key = response_string(setup, "recovery_key", "public API /api/vault/setup")
                self.redactor.secret(recovery_key)
                new_password = secrets.token_urlsafe(32)
                self.redactor.credentials(username, new_password)
                self.request("/api/vault/password", {"username": username, "password": password,
                                                     "new_password": new_password}, auth=False)
                password = new_password
            users.append({"url": self.url + "/dav/cal/" + username + "/",
                          "username": username, "password": password})
        return users

    def close(self):
        try:
            stop_child(self.child)
        finally:
            self.child = None
            self.log.close()


def run_suite(source, users, mode, work):
    policy = json.loads((HERE / "compatibility.json").read_text())[mode]
    features = validate_policy(policy, set(json.loads((HERE / "collection.json").read_text())))
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
                "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy",
                "PYTHON_CALDAV_COMMDUMP", "PYTHON_CALDAV_DEBUGMODE"):
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
    with open(work / "pytest.log", "xb") as output:
        os.chmod(work / "pytest.log", 0o600)
        result = subprocess.run(command + selection + [str(HERE / "test_flows.py")],
                                cwd=suite, env=env, stdout=output, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f"{mode}: third-party client tests failed (exit {result.returncode})")


def run_modes(binary, source, modes, logs=None):
    logs = log_directory(logs) if logs is not None else None
    failed = False
    try:
        for mode in modes:
            # Each mode owns its complete process/store lifetime, including bootstrap failures.
            with tempfile.TemporaryDirectory(prefix=f"stalwart-caldav-{mode}-") as raw:
                work = Path(raw)
                os.chmod(work, 0o700)
                server = None
                errors = []
                try:
                    server = ProductServer(binary, work)
                    server.bootstrap()
                    run_suite(source, server.users(mode), mode, work)
                except (RuntimeError, OSError, subprocess.SubprocessError) as error:
                    errors.append(str(error))
                finally:
                    if server is not None:
                        try:
                            server.close()
                        except (RuntimeError, OSError, subprocess.SubprocessError) as error:
                            errors.append("Cleanup: " + str(error))
                    redactor = server.redactor if server is not None else Redactor()
                    # Export only these two diagnostics, after exact redaction and process cleanup.
                    for name in ("server.log", "pytest.log"):
                        try:
                            path = work / name
                            content = path.read_text(errors="replace") if path.is_file() else ""
                            content = redactor.sanitize(content)
                            if logs is not None:
                                retain_log(logs, mode + "-" + name, content)
                            if name == "pytest.log" and content:
                                print(content, end="" if content.endswith("\n") else "\n")
                        except OSError as error:
                            errors.append("Diagnostics: " + str(error))
                    for error in errors:
                        message = f"CalDAV client CI ({mode}): " + redactor.sanitize(error)
                        print(message, file=sys.stderr)
                failed |= bool(errors)
                print(f"CalDAV client CI ({mode}): {'FAILED' if errors else 'passed'}")
    finally:
        if logs is not None:
            os.close(logs)
    return int(failed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--caldav-source", required=True, type=Path)
    parser.add_argument("--mode", choices=("plain", "key", "both"), default="both")
    parser.add_argument("--log-dir", type=Path, help="New private directory for sanitized logs")
    args = parser.parse_args()
    head = subprocess.check_output(["git", "-C", str(args.caldav_source), "rev-parse", "HEAD"],
                                   text=True).strip()
    if head != PIN:
        parser.error(f"python-caldav must be checked out at {PIN}")
    modes = ("plain", "key") if args.mode == "both" else (args.mode,)
    try:
        policies = json.loads((HERE / "compatibility.json").read_text())
        expected = set(json.loads((HERE / "collection.json").read_text()))
        for mode in modes:
            validate_policy(policies[mode], expected)
    except (ValueError, KeyError, TypeError, AttributeError) as error:
        parser.error(f"Invalid compatibility policy: {error}")
    return run_modes(args.binary, args.caldav_source, modes, args.log_dir)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"CalDAV client CI: {error}", file=sys.stderr)
        sys.exit(1)
