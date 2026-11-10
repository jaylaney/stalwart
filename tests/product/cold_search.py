# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Measure cold and immediate-repeat JMAP search after restarting a disposable OSS store."""
from __future__ import annotations

import argparse
import base64
from dataclasses import dataclass
import hashlib
from http.client import HTTPConnection, HTTPException
import json
import os
from pathlib import Path
import re
import signal
import smtplib
import stat
import subprocess
import sys
import time

from fresh_store import Gate, GateError, runner

BODY_WORD = "bodycanary"
QUERY = {"filter": {"text": BODY_WORD}, "limit": 1}
SEARCH_UNAVAILABLE = "Search for this account is not available right now; try again shortly."
FULL_COUNT, SMOKE_COUNT = 30_000, 5
PAGE_SIZE = 5_000
QUERY_TIMEOUT = 600
SETUP_TIMEOUT = 10_800
STOP_TIMEOUT = 60
DELIVERY_BATCH = 50
LMTP_COMMIT = re.compile(rb"2\.0\.0 Message queued with id [0-9a-f]{1,16}\.")
DOMAIN = "cold-search.test"
ACCOUNT = "search"
MAIL = "urn:ietf:params:jmap:mail"
CORE = "urn:ietf:params:jmap:core"
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
TOKEN = re.compile(r"[A-Za-z0-9_-]{1,255}\Z")
BUILD_COMMAND = [
    "cargo", "build", "--release", "--locked", "-p", "stalwart",
    "--no-default-features", "--features", "rocks",
]
SEARCH_DEFAULTS = {
    "ZA_SEARCH_MEM_MB": 1536,
    "ZA_SEARCH_THREAD_MEM_MB": 256,
    "ZA_SEARCH_ACCOUNT_MAX_MB": 512,
    "ZA_SEARCH_IDLE_SECS": 900,
    "ZA_SEARCH_PASS_CAP": 48,
    "ZA_SEARCH_PASS_MB": 32,
    "ZA_SEARCH_PASS_HIDDEN_KB": 32,
    "ZA_SEARCH_PASS_MS": 2000,
    "ZA_SEARCH_WAIT_SECS": 30,
    "ZA_SEARCH_SEGMENTS": 16,
    "ZA_SEARCH_CHECKPOINT": 1000,
    "ZA_SEARCH_SKIP_AFTER": 8,
    "ZA_SEARCH_ABSENT_RETRY_SECS": 10,
    "ZA_SEARCH_PAD_MIN": 65536,
}
DISABLED = {"else": "false"}
MTA_SETTINGS = {
    "MtaStageAuth": {"require": DISABLED, "saslMechanisms": DISABLED},
    "MtaStageRcpt": {"allowRelaying": DISABLED},
    "MtaStageData": {
        "maxMessages": {"else": str(DELIVERY_BATCH)},
        "enableSpamFilter": DISABLED, "dkimSignDomain": DISABLED,
        "addDeliveredToHeader": False,
        **{field: DISABLED for field in (
            "addAuthResultsHeader", "addDateHeader", "addMessageIdHeader",
            "addReceivedHeader", "addReceivedSpfHeader", "addReturnPathHeader",
        )},
    },
    "SenderAuth": {field: {"else": "disable"} for field in (
        "dkimVerify", "spfEhloVerify", "spfFromVerify", "arcVerify", "dmarcVerify",
        "reverseIpVerify",
    )},
}


def token(value):
    return isinstance(value, str) and TOKEN.fullmatch(value) is not None


def digest(value):
    return isinstance(value, str) and SHA256.fullmatch(value) is not None


def require(condition, message):
    if not condition:
        raise GateError(message)


@dataclass(frozen=True)
class Corpus:
    directory: Path
    sha256: str
    messages: list[dict]

    @property
    def count(self):
        return len(self.messages)

    def read(self, ordinal):
        entry = self.messages[ordinal]
        flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
        with os.fdopen(os.open(self.directory / entry["file"], flags), "rb") as source:
            info = os.fstat(source.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_size == entry["bytes"],
                    f"MIME size or file type differs at ordinal {ordinal}")
            raw = source.read(entry["bytes"] + 1)
        require(len(raw) == entry["bytes"] and hashlib.sha256(raw).hexdigest() == entry["sha256"],
                f"MIME checksum differs at ordinal {ordinal}")
        return raw

    @classmethod
    def verify(cls, directory, count):
        with (directory / "manifest.json").open("r", encoding="utf-8") as source:
            manifest = json.load(source)
        require(
            isinstance(manifest, dict)
            and all(type(manifest.get(field)) is int and manifest[field] == expected
                    for field, expected in (("version", 1), ("seed", 1), ("count", count)))
            and manifest.get("body_word") == BODY_WORD and digest(manifest.get("corpus_sha256"))
            and isinstance(manifest.get("messages"), list) and len(manifest["messages"]) == count,
            "manifest does not match the requested seed, size and export format",
        )
        corpus = cls(directory, manifest["corpus_sha256"], manifest["messages"])
        aggregate = hashlib.sha256()
        for ordinal, entry in enumerate(corpus.messages):
            require(
                isinstance(entry, dict) and entry.get("file") == f"{ordinal:06}.eml"
                and digest(entry.get("sha256"))
                and type(entry.get("bytes")) is int and 0 < entry["bytes"] <= 2 * 1024 * 1024
                and type(entry.get("attachment_bytes")) is int
                and 0 <= entry["attachment_bytes"] <= entry["bytes"],
                f"manifest entry is invalid at ordinal {ordinal}",
            )
            aggregate.update(corpus.read(ordinal))
        require(aggregate.hexdigest() == corpus.sha256, "corpus aggregate checksum differs")
        return corpus

    def summary(self):
        return {
            "version": 1, "seed": 1, "count": self.count, "sha256": self.sha256,
            "mime_bytes": sum(message["bytes"] for message in self.messages),
            "attachments": sum(message["attachment_bytes"] > 0 for message in self.messages),
            "attachment_bytes": sum(message["attachment_bytes"] for message in self.messages),
        }


class ProductServer(runner.ProductServer):
    """Normal HTTP and LMTP listeners bound to loopback on a fresh RocksDB store."""

    def provision(self):
        super().provision()
        # A disabled entry also prevents insertion of the two default inbound throttles.
        self.create("MtaInboundThrottle", {"description": "Disabled fixture throttle",
                    "enable": False, "key": {"remoteIp": True},
                    "rate": {"count": 1, "period": 1000}})
        for kind, values in MTA_SETTINGS.items():
            existing = self.objects(kind)
            if existing:
                self.update(kind, existing[0]["id"], values)
            else:
                self.create(kind, values)
        for tracer in self.objects("Tracer"):
            self.update("Tracer", tracer["id"], {
                "level": "info", "eventsPolicy": "include", "buffered": False,
                "events": {
                    "server.startup": True, "server.shutdown": True, "server.startup-error": True,
                    "registry.build-warning": True, "registry.build-error": True,
                    "calendar.itip-message-error": True,
                },
            })
        # Keep the HTTP port bound during recovery while choosing the separate LMTP port.
        self.lmtp_port = runner.port()
        self.create("NetworkListener", {
            "name": "cold-lmtp", "protocol": "lmtp",
            "bind": {f"127.0.0.1:{self.lmtp_port}": True}, "useTls": False,
        })

    def stop_clean(self):
        child = self.child
        require(child is not None and child.poll() is None,
                "the owned server was not running at clean stop")
        child.terminate()
        try:
            status = child.wait(timeout=STOP_TIMEOUT)
        except subprocess.TimeoutExpired:
            raise GateError("the owned server timed out during clean stop") from None
        self.child = None
        require(status == 0, f"the owned server exited with status {status}")

    def bootstrap(self):
        self.start(recovery=True)
        self.provision()
        self.stop_clean()
        self.start(recovery=False)

    def key_account(self, count, mime_bytes):
        domain = self.domain(DOMAIN)
        # Use the shared account setup and then override only this fixture's mail/disk quotas.
        auth = self.account(domain, DOMAIN, ACCOUNT, key=True)
        self.quotas = {"maxEmails": count + 100, "maxDiskQuota": 2 * mime_bytes + (1 << 30)}
        found = self.jmap("x:Account/query", {
            "filter": {"name": ACCOUNT, "domainId": domain},
        }).get("ids")
        require(isinstance(found, list) and len(found) == 1 and token(found[0]),
                "public registry did not identify the fixture account")
        self.update("Account", found[0], {"quotas": self.quotas})
        return auth


def smtp_reply(code, step, ordinal):
    if code != 250:
        raise GateError(f"LMTP {step} refused ordinal {ordinal} with status {int(code)}")


def deliver(corpus, port, recipient, receipt, save):
    """Record queue acknowledgments only. Mailbox commit is verified separately."""
    deadline = time.monotonic() + SETUP_TIMEOUT
    for start in range(0, corpus.count, DELIVERY_BATCH):
        try:
            with smtplib.LMTP("127.0.0.1", port, local_hostname=DOMAIN, timeout=60) as client:
                code, _ = client.ehlo()  # LMTP's EHLO command string is LHLO.
                smtp_reply(code, "LHLO", start)
                require(client.has_extn("chunking"), "LMTP listener did not advertise CHUNKING")
                for ordinal in range(start, min(start + DELIVERY_BATCH, corpus.count)):
                    require(time.monotonic() < deadline, "LMTP setup deadline expired")
                    raw = corpus.read(ordinal)
                    code, _ = client.mail("")
                    smtp_reply(code, "MAIL", ordinal)
                    code, _ = client.rcpt(recipient)
                    smtp_reply(code, "RCPT", ordinal)
                    client.putcmd("BDAT", f"{len(raw)} LAST")
                    client.send(raw)
                    code, reply = client.getreply()
                    smtp_reply(code, "BDAT", ordinal)
                    require(LMTP_COMMIT.fullmatch(reply) is not None,
                            f"LMTP did not acknowledge a queue commit at ordinal {ordinal}")
                    receipt["queued"] += 1
        except (OSError, smtplib.SMTPException) as error:
            raise GateError(f"LMTP transport failed after {receipt['queued']} acknowledgments "
                            f"({type(error).__name__})") from None
        finally:
            save()


class WireError(GateError):
    def __init__(self, kind, http_status=None):
        super().__init__(kind)
        self.kind = kind
        self.http_status = http_status


def request_json(port, path, auth, timeout, body=None):
    """A loopback request with no redirect, proxy, retry, or persisted response body."""
    authorization = base64.b64encode(":".join(auth).encode()).decode()
    headers = {"Authorization": "Basic " + authorization, "Content-Type": "application/json"}
    client = HTTPConnection("127.0.0.1", port, timeout=timeout)

    def expired(_signum, _frame):
        raise TimeoutError

    previous = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, timeout)
    try:
        client.request("GET" if body is None else "POST", path, body=body, headers=headers)
        response = client.getresponse()
        if response.status != 200:
            raise WireError("http-error", response.status)
        raw = response.read(2 * 1024 * 1024 + 1)
        if len(raw) > 2 * 1024 * 1024:
            raise WireError("invalid-response")
        result = json.loads(raw)
        if not isinstance(result, dict):
            raise WireError("invalid-response")
        return result
    except (ValueError, UnicodeError):
        raise WireError("invalid-response") from None
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
        client.close()


class Jmap:
    def __init__(self, port, auth):
        self.port = port
        self.auth = auth
        session = request_json(port, "/jmap/session", auth, 15)
        self.account_id = runner.response_string(session.get("primaryAccounts"), MAIL, "session")

    def __call__(self, method, arguments, timeout):
        result = request_json(self.port, "/jmap/", self.auth, timeout, json.dumps({
            "using": [CORE, MAIL],
            "methodCalls": [[method, {"accountId": self.account_id, **arguments}, "0"]],
        }))
        replies = result.get("methodResponses")
        if not isinstance(replies, list) or len(replies) != 1:
            raise WireError("invalid-response")
        reply = replies[0]
        if (not isinstance(reply, list) or len(reply) != 3 or reply[2] != "0"
                or not isinstance(reply[1], dict)):
            raise WireError("invalid-response")
        name, data, _ = reply
        if name == "error":
            description = data.get("description")
            if (data.get("type") == "unsupportedFilter" and isinstance(description, str)
                    and SEARCH_UNAVAILABLE in description):
                raise WireError("search-unavailable")
            raise WireError("jmap-error")
        if name != method or data.get("accountId") != self.account_id:
            raise WireError("invalid-response")
        return data


def query_fields(result, position, limit):
    ids = result.get("ids")
    state = result.get("queryState")
    if (not isinstance(ids, list) or len(ids) > limit or not all(map(token, ids))
            or not token(state) or type(result.get("position")) is not int
            or result["position"] != position):
        raise WireError("invalid-response")
    return ids, state


def id_digest(ids):
    return hashlib.sha256("\n".join(sorted(ids)).encode("ascii")).hexdigest()


class Probe:
    def __init__(self, jmap):
        self.jmap = jmap

    def one(self, timeout=QUERY_TIMEOUT):
        result = self.jmap("Email/query", QUERY, timeout=timeout)
        ids, state = query_fields(result, 0, 1)
        return {"ids": ids, "query_state": state}

    def attempt(self):
        began = time.monotonic()
        try:
            receipt = {"status": "ok", **self.one()}
        except WireError as error:
            receipt = {"status": error.kind}
            if error.http_status is not None:
                receipt["http_status"] = error.http_status
        except (OSError, HTTPException) as error:
            receipt = {"status": "transport-error", "reason": (
                "timeout" if isinstance(error, TimeoutError) else type(error).__name__)}
        return {**receipt, "seconds": time.monotonic() - began}

    def measure_pair(self, outcomes):
        outcomes.append(self.attempt())
        outcomes.append(self.attempt())

    def census(self, arguments, count):
        ids, state, first_id = set(), None, None
        for position in range(0, count, PAGE_SIZE):
            size = min(PAGE_SIZE, count - position)
            result = self.jmap("Email/query", {
                **arguments, "limit": size, "position": position, "calculateTotal": True,
            }, timeout=QUERY_TIMEOUT)
            page, page_state = query_fields(result, position, size)
            require(type(result.get("total")) is int and result["total"] == count
                    and len(page) == size and len(set(page)) == size and ids.isdisjoint(page),
                    "public census count or unique ID check failed")
            require(state is None or state == page_state, "public census changed during paging")
            state = page_state
            if position == 0:
                first_id = page[0]
            ids.update(page)
        return ids, state, first_id

    def coverage(self, count):
        mailbox, mail_state, _ = self.census({}, count)
        matched, search_state, first_id = self.census({"filter": QUERY["filter"]}, count)
        require(mailbox == matched and mail_state == search_state,
                "public body canary coverage differs from the mailbox census")
        return {"count": len(mailbox), "ids_sha256": id_digest(mailbox),
                "query_state": search_state, "first_id": first_id}

    def prepare(self, count, receipt, save):
        # Content queries drive ordinary durable indexing passes. Retry only the known
        # unavailable method response during setup. The measured pair has no retries.
        deadline = time.monotonic() + SETUP_TIMEOUT
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0, "setup body search did not converge within the setup deadline")
            try:
                expected = self.one(timeout=min(QUERY_TIMEOUT, remaining))
                break
            except WireError as error:
                if error.kind != "search-unavailable":
                    raise
                receipt["setup_search_refusals"] += 1
                save()
        require(len(expected["ids"]) == 1, "setup limit-1 query did not return one ID")
        receipt["expected"] = expected
        coverage = receipt["before_restart"] = self.coverage(count)
        require(expected["query_state"] == coverage["query_state"],
                "setup public query state changed")
        require(expected["ids"] == [coverage["first_id"]],
                "setup limit-1 identity differs from the filtered census")
        save()
        return expected


def restart_and_measure(server, probe, expected, receipt, save):
    server.stop_clean()
    receipt["clean_stop"] = True
    save()
    server.start(recovery=False)  # The shared readiness path uses only plain admin authentication.
    try:
        probe.measure_pair(receipt["pair"])
    finally:
        save()
    require(all(result["status"] == "ok" and result["ids"] == expected["ids"]
                and result["query_state"] == expected["query_state"]
                for result in receipt["pair"]) and len(receipt["pair"]) == 2,
            "measured cold and repeat query pair did not match the baseline")


def file_digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()


def binary_identity(binary, expected_sha256, source_commit):
    checksum = file_digest(binary)
    require(checksum == expected_sha256, "binary checksum differs from the shipping build receipt")
    result = subprocess.run(
        [str(binary), "--version"], env={"PATH": os.defpath}, capture_output=True,
        text=True, timeout=30, check=True,
    )
    identity = result.stdout.strip()
    match = re.fullmatch(
        r"Stalwart Server fork v[0-9A-Za-z.+-]+ \(upstream v[0-9A-Za-z.+-]+ "
        r"[0-9a-f]{40}; source ([0-9a-f]{40}); clean\)", identity,
    )
    require(match is not None and match[1] == source_commit,
            "binary version does not report the expected clean source")
    return {"sha256": checksum, "version": identity, "source_commit": source_commit,
            "expected_build_command": BUILD_COMMAND}


def check_startup(path, expected_starts):
    defaults = " ".join(f"{key}={value}" for key, value in SEARCH_DEFAULTS.items())
    starts = 0
    with path.open(encoding="utf-8", errors="replace") as lines:
        for line in lines:
            require("Built with test_mode:" not in line, "server startup reported test_mode")
            if "Zero-access search" in line and defaults in line:
                starts += 1
    require(starts == expected_starts, "normal startup did not report all fourteen search defaults")


def retain_server_log(work, server):
    raw = work / "store" / "server.log"
    with raw.open(encoding="utf-8", errors="replace") as source:
        with (work / "server.sanitized.log").open("x", encoding="utf-8") as output:
            os.chmod(output.fileno(), 0o600)
            for line in source:
                output.write(server.redactor.sanitize(line))
    raw.unlink()


def run(args, command):
    work = args.work_dir.absolute()
    os.close(runner.log_directory(work))
    receipt = {"mode": "five-message-smoke" if args.smoke else "full-30000",
               "command": command, "phase": "verify inputs", "status": "failed",
               "queued": 0, "setup_search_refusals": 0, "pair": []}

    def save():
        temporary = work / "receipt.next.json"
        with temporary.open("w", encoding="utf-8") as output:
            os.chmod(output.fileno(), 0o600)
            json.dump(receipt, output, indent=2)
            output.write("\n")
        temporary.replace(work / "receipt.json")

    def step(phase):
        receipt["phase"] = phase
        save()
        print("cold search: " + phase, flush=True)

    began = time.monotonic()
    server = None
    try:
        step("verify inputs")
        count = SMOKE_COUNT if args.smoke else FULL_COUNT
        corpus = Corpus.verify(args.corpus, count)
        binary = args.binary.resolve()
        receipt["corpus"] = corpus.summary()
        receipt["binary"] = binary_identity(binary, args.binary_sha256, args.source_commit)
        directory = work / "store"
        directory.mkdir(mode=0o700)
        server = ProductServer(binary, directory)
        receipt["server_command"] = [str(binary), "--config", str(server.config)]
        step("provision disposable RocksDB store")
        server.bootstrap()
        check_startup(directory / "server.log", 1)
        auth = server.key_account(count, receipt["corpus"]["mime_bytes"])
        receipt["settings"] = {
            "recovery_bind": f"[::]:{server.http_port} (IPv4 wildcard fallback)",
            "normal_http_bind": f"127.0.0.1:{server.http_port}",
            "normal_lmtp_bind": f"127.0.0.1:{server.lmtp_port}",
            "search_defaults": SEARCH_DEFAULTS, "account_quotas": server.quotas,
            "delivery": "LMTP BDAT, one local recipient, null envelope sender",
            "mta_settings": MTA_SETTINGS, "inbound_throttle_enabled": False,
            "applications_enabled": False, "spam_enabled": False, "asn_enabled": False,
        }
        step("submit verified fixture via LMTP")
        deliver(corpus, server.lmtp_port, auth[0], receipt, save)
        require(receipt["queued"] == count, "LMTP acknowledgment count differs from the fixture")
        step("drain public delivery and task queues")
        Gate(server, lambda _message: None).wait_for_drain(timeout=SETUP_TIMEOUT)
        step("verify complete public body canary coverage before restart")
        probe = Probe(Jmap(server.http_port, auth))
        expected = probe.prepare(count, receipt, save)
        step("restart and send the two measured queries")
        restart_and_measure(server, probe, expected, receipt, save)
        step("verify public coverage after the pair")
        receipt["after_restart"] = probe.coverage(count)
        require(receipt["after_restart"] == receipt["before_restart"],
                "public coverage or query state changed across restart")
        check_startup(directory / "server.log", 2)
        require(file_digest(binary) == args.binary_sha256, "binary changed during the run")
        server.stop_clean()
        receipt["phase"] = "complete"
        receipt["status"] = "passed"
    except GateError as error:
        # GateError messages in this runner and the shared drain contain no reply bodies.
        receipt["error"] = str(error) if server is None else server.redactor.sanitize(str(error))
    except (Exception, KeyboardInterrupt) as error:
        # Transport and library exceptions may hold URLs, credentials or response bodies.
        receipt["error"] = type(error).__name__
    finally:
        if server is not None:
            try:
                server.close()
                retain_server_log(work, server)
            except Exception as error:
                receipt["status"] = "failed"
                receipt["cleanup_error"] = type(error).__name__
        receipt["seconds"] = time.monotonic() - began
        save()
    print(f"cold search: {receipt['mode']} {receipt['status']}", flush=True)
    return int(receipt["status"] != "passed")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--binary-sha256", required=True, help="SHA-256 from the shipping build")
    parser.add_argument("--source-commit", required=True,
                        help="Clean commit from the shipping build")
    parser.add_argument("--corpus", required=True, type=Path,
                        help="Directory from the Rust exporter")
    parser.add_argument("--work-dir", required=True, type=Path,
                        help="New private evidence and store directory")
    parser.add_argument("--smoke", action="store_true",
                        help="Require exactly five exported messages")
    args = parser.parse_args(argv)
    if not digest(args.binary_sha256) or not COMMIT.fullmatch(args.source_commit):
        parser.error("shipping binary SHA-256 and source commit must be lowercase hex")
    try:
        command = [sys.executable, *sys.argv] if argv is None else [sys.executable, __file__, *argv]
        return run(args, command)
    except (OSError, ValueError) as error:
        print(f"cold search: evidence directory failed ({type(error).__name__})", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
