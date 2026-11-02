# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Create a fresh store with the shipping binary, restart it, and read every seed back.

The gate uses public interfaces only: the registry JMAP API as the recovery administrator,
the vault, contacts and share endpoints, CalDAV, JMAP, IMAP over implicit TLS and SMTP
submission over implicit TLS. It starts the server exactly as the independent CalDAV runner
does (tests/caldav_client/runner.py: ProductServer) and adds its own mail listeners.

It prints no credential, key or token, and the logs it keeps are redacted the same way.
"""
from __future__ import annotations

import argparse
import base64
import email
import email.message
import email.policy
import email.utils
import imaplib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import signal
import smtplib
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "caldav_client"))
import runner  # noqa: E402  (the shared server lifecycle)

DOMAIN = "fresh-store.test"
ORDINARY, ALPHA, BETA = "ordinary", "keyalpha", "keybeta"
PREFS_CALENDAR = "fresh-prefs"
SHARED_CALENDAR = "fresh-shared"
ZA_MAIL_KEYLESS = ("Mail in this account is encrypted at rest and is not available to "
                   "this session.")
DAV = "DAV:"
CALDAV = "urn:ietf:params:xml:ns:caldav"
APPLE = "http://apple.com/ns/ical/"
PREFERENCES = {"displayname": "Fresh store preferences",
               "calendar-description": "Preferences kept across a restart",
               "calendar-timezone-id": "Europe/Paris",
               "calendar-color": "#3A7BD5FF"}
# Every check the readback must pass; a check that never reports is a failure.
CHECKS = (
    "mail: ordinary account (SMTP delivery and IMAP APPEND)",
    "mail: key accounts (SMTP delivery and IMAP APPEND)",
    "attachments: nested message's binary attachment, byte for byte",
    "calendar preferences",
    "recurrence with an override",
    "pins",
    "sharing: permissions after restart",
    "sharing: revoke and regrant after restart",
    "keyless refusal",
)
DEFAULT_TIMEOUT = 900
DRAIN_TIMEOUT = 180
STOP_TIMEOUT = 60


class GateError(RuntimeError):
    """A failed step or check; its message never holds a response body or secret."""


class GateTimeout(BaseException):
    """The run's deadline; a BaseException so that no retry loop can swallow it."""


def tls_context():
    # The server falls back to its own self-signed certificate on a fresh store.
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    return context


def http(url, method="GET", body=None, headers=None, auth=None, timeout=15):
    """One request on a new connection; returns the status and body, never raises on 4xx."""
    headers = dict(headers or {})
    if auth is not None:
        headers["Authorization"] = "Basic " + base64.b64encode(
            ":".join(auth).encode()).decode()
    data = body.encode() if isinstance(body, str) else body
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(request, timeout=timeout) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        try:
            return error.code, error.read()
        finally:
            error.close()
    except (urllib.error.URLError, OSError) as error:
        raise GateError(f"{method} request failed: {type(error).__name__}") from None


def expect(condition, message):
    if not condition:
        raise GateError(message)


def unfold(text):
    return re.sub(r"\r?\n[ \t]", "", text).splitlines()


def components(ical, name="VEVENT"):
    """The property lines (name without parameters -> value) of each `name` component."""
    found, current = [], None
    for line in unfold(ical):
        if line == f"BEGIN:{name}":
            current = {}
        elif line == f"END:{name}" and current is not None:
            found.append(current)
            current = None
        elif current is not None and ":" in line:
            key, value = line.split(":", 1)
            current.setdefault(key.split(";", 1)[0].upper(), value)
    return found


def series(uid, token):
    stamp = "20261101T000000Z"
    return "\r\n".join([
        "BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Stalwart fork//fresh-store gate//EN",
        "BEGIN:VEVENT", f"UID:{uid}", f"DTSTAMP:{stamp}", "DTSTART:20300311T140000Z",
        "DTEND:20300311T150000Z", "RRULE:FREQ=DAILY;COUNT=5",
        f"SUMMARY:Fresh series {token}", "SEQUENCE:0", "END:VEVENT",
        "BEGIN:VEVENT", f"UID:{uid}", f"DTSTAMP:{stamp}", "RECURRENCE-ID:20300313T140000Z",
        "DTSTART:20300313T170000Z", "DTEND:20300313T180000Z",
        f"SUMMARY:Fresh override {token}", "SEQUENCE:0", "END:VEVENT",
        "END:VCALENDAR", ""])


def single(uid, summary):
    return "\r\n".join([
        "BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Stalwart fork//fresh-store gate//EN",
        "BEGIN:VEVENT", f"UID:{uid}", "DTSTAMP:20261101T000000Z",
        "DTSTART:20300401T090000Z", "DTEND:20300401T100000Z", f"SUMMARY:{summary}",
        "SEQUENCE:0", "END:VEVENT", "END:VCALENDAR", ""])


EXPANDED_STARTS = ["20300311T140000Z", "20300312T140000Z", "20300313T170000Z",
                   "20300314T140000Z", "20300315T140000Z"]


def binary_attachment(label):
    """Every byte value, then random bytes: a payload that a text path would damage."""
    return bytes(range(256)) * 4 + label.encode() + secrets.token_bytes(2048)


def nested_message(sender, recipient, subject, payload):
    """multipart/mixed: text, then a message/rfc822 whose own multipart holds the bytes."""
    inner = email.message.EmailMessage(policy=email.policy.SMTP)
    inner["From"] = sender
    inner["To"] = recipient
    inner["Subject"] = "Inner " + subject
    inner["Message-ID"] = f"<{uuid.uuid4().hex}@{DOMAIN}>"
    inner.set_content("The attached binary must survive a restart byte for byte.\r\n")
    inner.add_attachment(payload, maintype="application", subtype="octet-stream",
                         filename="payload.bin")
    outer = email.message.EmailMessage(policy=email.policy.SMTP)
    outer["From"] = sender
    outer["To"] = recipient
    outer["Subject"] = subject
    outer["Date"] = email.utils.formatdate(localtime=False)
    outer["Message-ID"] = f"<{uuid.uuid4().hex}@{DOMAIN}>"
    outer.set_content("A nested message with a binary attachment follows.\r\n")
    outer.add_attachment(inner)
    return outer.as_bytes()


def nested_payload(raw):
    """The nested message's binary attachment, decoded from a fetched message."""
    outer = email.message_from_bytes(raw, policy=email.policy.default)
    for part in outer.walk():
        if part.get_content_type() == "message/rfc822":
            inner = part.get_payload()[0]
            for leaf in inner.walk():
                if leaf.get_filename() == "payload.bin":
                    return leaf.get_payload(decode=True)
    return None


def subject_of(raw):
    header = email.message_from_bytes(raw, policy=email.policy.default)["Subject"]
    return str(header) if header is not None else None


class FreshServer(runner.ProductServer):
    """runner.ProductServer with IMAP and SMTP submission listeners on implicit TLS."""

    def provision(self):
        super().provision()
        self.imap_port = runner.port()
        self.smtp_port = runner.port()
        self.create("NetworkListener", {"name": "fresh-imaps", "protocol": "imap",
                    "bind": {f"127.0.0.1:{self.imap_port}": True},
                    "useTls": True, "implicitTls": True})
        self.create("NetworkListener", {"name": "fresh-submissions", "protocol": "smtp",
                    "bind": {f"127.0.0.1:{self.smtp_port}": True},
                    "useTls": True, "implicitTls": True})

    def stop_clean(self, timeout=STOP_TIMEOUT):
        child, self.child = self.child, None
        if child is None or child.poll() is not None:
            raise GateError("server was not running when it was to be stopped")
        child.send_signal(signal.SIGTERM)
        try:
            status = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            runner.stop_child(child)
            raise GateError(f"server did not stop within {timeout} seconds") from None
        if status != 0:
            raise GateError(f"server exited with status {status} when stopped")

    def restart(self):
        self.stop_clean()
        self.start(recovery=False)


class Gate:
    def __init__(self, server, log):
        self.server = server
        self.log = log
        self.users = {}
        self.mail_ids = {}
        self.expected_mail = {}
        self.calendars = {}
        self.pins = {}
        self.shares = {}
        self.passed = []

    # Public-interface helpers.
    def user(self, name):
        return self.users[name]

    def dav(self, who, path, method="GET", body=None, headers=None):
        auth = self.server.admin if who == "admin" else self.user(who)
        return http(self.server.url + path, method, body, headers, auth)

    def api(self, who, endpoint, **fields):
        username, password = self.user(who)
        status, body = http(self.server.url + "/api/vault/" + endpoint, "POST",
                            json.dumps({"username": username, "password": password,
                                        **fields}),
                            {"Content-Type": "application/json"})
        expect(status == 200, f"/api/vault/{endpoint} for {who} answered HTTP {status}")
        try:
            data = json.loads(body)
        except ValueError:
            raise GateError(f"/api/vault/{endpoint} returned invalid JSON") from None
        expect(isinstance(data, dict), f"/api/vault/{endpoint} returned a non-object")
        return data

    def jmap(self, auth, account_id, method, arguments):
        status, body = http(self.server.url + "/jmap/", "POST", json.dumps({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [[method, {"accountId": account_id, **arguments}, "0"]],
        }), {"Content-Type": "application/json"}, auth)
        expect(status == 200, f"JMAP {method} answered HTTP {status}")
        replies = json.loads(body).get("methodResponses")
        expect(isinstance(replies, list) and len(replies) == 1 and len(replies[0]) == 3,
               f"JMAP {method} returned an invalid response")
        return replies[0][0], replies[0][1]

    def imap(self, who):
        connection = imaplib.IMAP4_SSL("127.0.0.1", self.server.imap_port,
                                       ssl_context=tls_context(), timeout=30)
        try:
            connection.login(*self.user(who))
        except imaplib.IMAP4.error:
            connection.shutdown()
            raise GateError(f"IMAP login refused for {who}") from None
        return connection

    def calendar_path(self, who, calendar):
        return f"/dav/cal/{self.user(who)[0]}/{calendar}/"

    def passed_check(self, name):
        assert name in CHECKS, name
        self.passed.append(name)
        self.log(f"check passed: {name}")

    # Create.
    def create(self):
        domain = self.server.domain(DOMAIN)
        for name, key in ((ORDINARY, False), (ALPHA, True), (BETA, True)):
            self.users[name] = self.server.account(domain, DOMAIN, name, key=key)
            self.log(f"created {'key' if key else 'ordinary'} account {name}"
                     + (" and completed its key setup" if key else ""))
        for name in self.users:
            status, body = http(self.server.url + "/jmap/session", auth=self.user(name))
            expect(status == 200, f"JMAP session for {name} answered HTTP {status}")
            primary = json.loads(body).get("primaryAccounts", {})
            self.mail_ids[name] = runner.response_string(
                primary, "urn:ietf:params:jmap:mail", "JMAP session")
        self.create_pins()
        self.create_mail()
        self.create_calendars()
        self.create_sharing()

    def create_pins(self):
        for owner, contact in ((ALPHA, BETA), (BETA, ALPHA)):
            found = self.api(owner, "contacts/lookup", address=self.user(contact)[0])
            fingerprint = found.get("fingerprint")
            expect(isinstance(fingerprint, str) and fingerprint,
                   "contacts/lookup returned no fingerprint")
            pinned = self.api(owner, "contacts/pin", address=self.user(contact)[0],
                              fingerprint=fingerprint)
            expect(pinned.get("ok") is True, f"contacts/pin for {owner} did not succeed")
            self.pins[owner] = (contact, fingerprint)
        self.log("pinned the two key accounts to each other")

    def create_mail(self):
        sender = self.user(ORDINARY)[0]
        recipients = [self.user(name)[0] for name in self.users]
        token = uuid.uuid4().hex
        subject = f"Fresh SMTP {token}"
        payload = binary_attachment("smtp")
        raw = nested_message(sender, ", ".join(recipients), subject, payload)
        try:
            with smtplib.SMTP_SSL("127.0.0.1", self.server.smtp_port,
                                  context=tls_context(), timeout=30) as smtp:
                smtp.login(*self.user(ORDINARY))
                refused = smtp.sendmail(sender, recipients, raw)
        except (smtplib.SMTPException, OSError) as error:
            raise GateError(f"SMTP submission failed: {type(error).__name__}") from None
        expect(not refused, "SMTP submission refused a recipient")
        for name in self.users:
            self.expected_mail[name] = {"smtp": (subject, payload)}
        self.log("submitted one SMTP message with a nested binary attachment to all accounts")
        for name in self.users:
            subject = f"Fresh APPEND {name} {uuid.uuid4().hex}"
            payload = binary_attachment("append-" + name)
            raw = nested_message(self.user(name)[0], self.user(name)[0], subject, payload)
            connection = self.imap(name)
            try:
                status, _ = connection.append("INBOX", None, None, raw)
                expect(status == "OK", f"IMAP APPEND for {name} answered {status}")
            finally:
                connection.logout()
            self.expected_mail[name]["append"] = (subject, payload, raw)
        self.log("appended one message with a nested binary attachment per account by IMAP")

    def create_calendars(self):
        for name in self.users:
            path = self.calendar_path(name, PREFS_CALENDAR)
            status, _ = self.dav(name, path, "MKCALENDAR")
            expect(status == 201, f"MKCALENDAR for {name} answered HTTP {status}")
            status, body = self.dav(name, path, "PROPPATCH", proppatch(PREFERENCES))
            expect(status == 207 and not failed_propstat(body),
                   f"PROPPATCH of the preferences for {name} failed (HTTP {status})")
            uid = uuid.uuid4().hex
            token = uuid.uuid4().hex
            status, _ = self.dav(name, path + "series.ics", "PUT", series(uid, token),
                                 {"Content-Type": "text/calendar", "If-None-Match": "*"})
            expect(status == 201, f"PUT of the recurring event for {name} answered {status}")
            self.calendars[name] = (uid, token)
        self.log("created a calendar with preferences and a recurring event with an "
                 "override per account")

    def create_sharing(self):
        path = self.calendar_path(ALPHA, SHARED_CALENDAR)
        status, _ = self.dav(ALPHA, path, "MKCALENDAR")
        expect(status == 201, f"MKCALENDAR of the shared calendar answered HTTP {status}")
        status, _ = self.dav(ALPHA, path + "owner.ics", "PUT",
                             single(uuid.uuid4().hex, "Owner event"),
                             {"Content-Type": "text/calendar"})
        expect(status == 201, f"owner PUT answered HTTP {status}")
        self.grant("view")
        self.expect_grantee(path, read=True, write=False, label="view grant")
        self.grant("edit")
        self.expect_grantee(path, read=True, write=True, label="edit grant")
        self.revoke()
        self.expect_grantee(path, read=False, write=False, label="revoke")
        self.grant("view")
        self.expect_grantee(path, read=True, write=False, label="regrant (view)")
        self.log("shared a calendar: view, edit, revoke and regrant (view) behaved")

    def grant(self, level):
        changed = self.api(ALPHA, "shares/grant", calendar=SHARED_CALENDAR,
                           address=self.user(BETA)[0], level=level)
        expect(changed.get("ok") is True and changed.get("changed") is True,
               f"shares/grant ({level}) did not change access")

    def revoke(self):
        changed = self.api(ALPHA, "shares/revoke", calendar=SHARED_CALENDAR,
                           address=self.user(BETA)[0])
        expect(changed.get("ok") is True and changed.get("changed") is True,
               "shares/revoke did not change access")

    def expect_grantee(self, path, read, write, label):
        status, body = self.dav(BETA, path + "owner.ics")
        if read:
            expect(status == 200 and b"SUMMARY:Owner event" in body,
                   f"{label}: the grantee's read answered HTTP {status}")
        else:
            expect(status in (403, 404), f"{label}: the grantee's read answered HTTP {status}")
        name = f"grantee-{uuid.uuid4().hex}.ics"
        summary = f"Grantee write {uuid.uuid4().hex}"
        status, _ = self.dav(BETA, path + name, "PUT", single(uuid.uuid4().hex, summary),
                             {"Content-Type": "text/calendar"})
        if write:
            expect(status == 201, f"{label}: the grantee's write answered HTTP {status}")
            status, body = self.dav(ALPHA, path + name)
            expect(status == 200 and summary.encode() in body,
                   f"{label}: the owner's read of the grantee's write answered {status}")
        else:
            expect(status == 403, f"{label}: the grantee's write answered HTTP {status}")

    def snapshot_pins(self):
        return {owner: sorted(self.api(owner, "contacts").get("pins") or [],
                              key=lambda pin: pin.get("address") or "")
                for owner in (ALPHA, BETA)}

    def snapshot_shares(self):
        return {who: self.api(who, "shares") for who in (ALPHA, BETA)}

    # Restart.
    def wait_for_drain(self, timeout=DRAIN_TIMEOUT):
        """Until the SMTP queue is empty, the index queues hold nothing and no task is due.

        A task scheduled for later (a share-revocation cleanup is due minutes after the
        revoke, maintenance tasks recur) is not work in flight: it stays in the store and is
        carried across the restart. A failed or stalled task fails the gate.
        """
        deadline = time.monotonic() + timeout
        state = "not checked"
        while time.monotonic() < deadline:
            queued = self.server.jmap("x:QueuedMessage/query", {}).get("ids")
            indexes = self.server.objects("IndexQueueStatus")
            tasks = self.server.objects("Task")
            now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            pending_index = [entry for entry in indexes
                             if entry.get("queuedUpdates") or entry.get("queuedDeletions")
                             or (entry.get("status") or {}).get("@type") == "Failed"]
            broken = sorted(task_kind(task) for task in tasks
                            if task_status(task) in ("Failed", "Stalled"))
            if broken:
                raise GateError("tasks failed before the restart: " + ", ".join(broken))
            due = sorted(task_kind(task) for task in tasks
                         if not isinstance(task.get("due"), str) or task["due"] <= now)
            if queued == [] and not pending_index and not due:
                later = sorted(task_kind(task) for task in tasks)
                self.log("work drained: SMTP queue empty, index queues empty, no task due"
                         + (f"; scheduled for later: {', '.join(later)}" if later else ""))
                return
            state = (f"{len(queued or [])} queued messages, {len(pending_index)} index "
                     f"queues with work, tasks due: {', '.join(due) or 'none'}")
            time.sleep(0.5)
        raise GateError(f"work did not drain within {timeout} seconds ({state})")

    # Read back.
    def readback(self, pins_before, shares_before):
        self.check_mail()
        self.check_calendars()
        expect(self.snapshot_pins() == pins_before, "pins differ after the restart")
        for owner, (contact, fingerprint) in self.pins.items():
            pins = {pin.get("address"): pin for pin in pins_before[owner]}
            pin = pins.get(self.user(contact)[0])
            expect(pin is not None and pin.get("fingerprint") == fingerprint,
                   f"{owner}'s pin of {contact} is missing or holds another key")
        self.passed_check("pins")
        self.check_sharing(shares_before)
        self.check_keyless()

    def check_mail(self):
        for group, names in (("ordinary", (ORDINARY,)), ("key", (ALPHA, BETA))):
            for name in names:
                messages = self.fetch_all(name)
                expected = self.expected_mail[name]
                subject, payload, raw = expected["append"]
                found = [message for message in messages if subject_of(message) == subject]
                expect(len(found) == 1, f"{name}: the IMAP APPEND message is missing")
                expect(found[0] == raw, f"{name}: the IMAP APPEND message changed")
                expect(nested_payload(found[0]) == payload,
                       f"{name}: the APPEND attachment changed")
                subject, payload = expected["smtp"]
                found = [message for message in messages if subject_of(message) == subject]
                expect(len(found) == 1, f"{name}: the SMTP-delivered message is missing")
                expect(nested_payload(found[0]) == payload,
                       f"{name}: the SMTP attachment changed")
            label = ("mail: ordinary account (SMTP delivery and IMAP APPEND)"
                     if group == "ordinary"
                     else "mail: key accounts (SMTP delivery and IMAP APPEND)")
            self.passed_check(label)
        self.passed_check("attachments: nested message's binary attachment, byte for byte")

    def fetch_all(self, name):
        connection = self.imap(name)
        try:
            status, _ = connection.select("INBOX", readonly=True)
            expect(status == "OK", f"{name}: IMAP SELECT answered {status}")
            status, data = connection.uid("FETCH", "1:*", "(BODY.PEEK[])")
            expect(status == "OK", f"{name}: IMAP FETCH answered {status}")
            return [item[1] for item in data if isinstance(item, tuple)]
        finally:
            connection.logout()

    def check_calendars(self):
        for name in self.users:
            path = self.calendar_path(name, PREFS_CALENDAR)
            status, body = self.dav(name, path, "PROPFIND", propfind(PREFERENCES),
                                    {"Depth": "0"})
            expect(status == 207, f"{name}: PROPFIND of the preferences answered {status}")
            values = prop_values(body, PREFERENCES)
            for prop, value in PREFERENCES.items():
                expect(values.get(prop) == value, f"{name}: calendar preference {prop} changed")
        self.passed_check("calendar preferences")
        for name in self.users:
            uid, token = self.calendars[name]
            path = self.calendar_path(name, PREFS_CALENDAR)
            status, body = self.dav(name, path + "series.ics")
            expect(status == 200, f"{name}: GET of the recurring event answered {status}")
            events = components(body.decode())
            master = [event for event in events if "RECURRENCE-ID" not in event]
            override = [event for event in events if "RECURRENCE-ID" in event]
            expect(len(master) == 1 and len(override) == 1,
                   f"{name}: the series lost its master or its override")
            expect(master[0].get("RRULE") == "FREQ=DAILY;COUNT=5"
                   and master[0].get("SUMMARY") == f"Fresh series {token}"
                   and master[0].get("UID") == uid,
                   f"{name}: the series' master changed")
            expect(override[0].get("RECURRENCE-ID") == "20300313T140000Z"
                   and override[0].get("DTSTART") == "20300313T170000Z"
                   and override[0].get("SUMMARY") == f"Fresh override {token}",
                   f"{name}: the series' override changed")
            status, body = self.dav(name, path, "REPORT", expand_query(), {"Depth": "1"})
            expect(status == 207, f"{name}: the expanded calendar-query answered {status}")
            starts = sorted(event.get("DTSTART") for event in components(
                "\n".join(calendar_data(body))))
            expect(starts == EXPANDED_STARTS,
                   f"{name}: the server expanded the series to {len(starts)} other instances")
        self.passed_check("recurrence with an override")

    def check_sharing(self, before):
        after = self.snapshot_shares()
        expect(after == before, "the share listings differ after the restart")
        path = self.calendar_path(ALPHA, SHARED_CALENDAR)
        own = [calendar for calendar in after[ALPHA].get("calendars", [])
               if calendar.get("calendar") == SHARED_CALENDAR]
        grants = own[0].get("grants", []) if own else []
        expect(len(grants) == 1 and grants[0].get("address") == self.user(BETA)[0]
               and grants[0].get("level") == "view" and grants[0].get("state") == "active",
               "the owner's listing does not hold the grantee's active view grant")
        shared = [entry for entry in after[BETA].get("shared_with_me", [])
                  if entry.get("calendar") == SHARED_CALENDAR]
        expect(len(shared) == 1 and shared[0].get("owner") == self.user(ALPHA)[0]
               and shared[0].get("level") == "view",
               "the grantee's listing does not hold the owner's calendar at view")
        self.expect_grantee(path, read=True, write=False, label="view after restart")
        self.passed_check("sharing: permissions after restart")
        self.revoke()
        self.expect_grantee(path, read=False, write=False, label="revoke after restart")
        self.grant("edit")
        self.expect_grantee(path, read=True, write=True, label="regrant (edit) after restart")
        self.passed_check("sharing: revoke and regrant after restart")

    def check_keyless(self):
        admin = self.server.admin
        properties = {"ids": None, "properties": ["subject"]}
        method, result = self.jmap(admin, self.mail_ids[ORDINARY], "Email/get", properties)
        expect(method == "Email/get" and len(result.get("list", [])) == 2,
               "control: the administrator's session does not read the ordinary mail")
        for name in (ALPHA, BETA):
            method, result = self.jmap(admin, self.mail_ids[name], "Email/get", properties)
            expect(method == "error" and result.get("type") == "forbidden"
                   and result.get("description") == ZA_MAIL_KEYLESS,
                   f"a keyless session read {name}'s mail")
        status, _ = self.dav("admin", self.calendar_path(ORDINARY, PREFS_CALENDAR),
                             "PROPFIND", propfind(PREFERENCES), {"Depth": "0"})
        expect(status == 207, f"control: the administrator's calendar read answered {status}")
        for name in (ALPHA, BETA):
            status, _ = self.dav("admin", self.calendar_path(name, PREFS_CALENDAR),
                                 "PROPFIND", propfind(PREFERENCES), {"Depth": "0"})
            expect(status == 403, f"a keyless session's read of {name}'s calendar "
                   f"answered HTTP {status}")
        self.passed_check("keyless refusal")


def task_status(task):
    return (task.get("status") or {}).get("@type")


def task_kind(task):
    return f"{task.get('@type')} ({task_status(task)})"


def proppatch(values):
    return (f'<D:propertyupdate xmlns:D="{DAV}" xmlns:C="{CALDAV}" xmlns:A="{APPLE}">'
            "<D:set><D:prop>"
            + "".join(f"<{prefix(name)}:{name}>{value}</{prefix(name)}:{name}>"
                      for name, value in values.items())
            + "</D:prop></D:set></D:propertyupdate>")


def propfind(values):
    return (f'<D:propfind xmlns:D="{DAV}" xmlns:C="{CALDAV}" xmlns:A="{APPLE}"><D:prop>'
            + "".join(f"<{prefix(name)}:{name}/>" for name in values)
            + "</D:prop></D:propfind>")


def prefix(name):
    return {"displayname": "D", "calendar-color": "A"}.get(name, "C")


def namespace(name):
    return {"D": DAV, "C": CALDAV, "A": APPLE}[prefix(name)]


def parse_xml(body):
    import xml.etree.ElementTree as ElementTree
    try:
        return ElementTree.fromstring(body)
    except ElementTree.ParseError:
        raise GateError("the server returned malformed XML") from None


def failed_propstat(body):
    root = parse_xml(body)
    return any(" 200 " not in (status.text or "")
               for status in root.iter(f"{{{DAV}}}status"))


def prop_values(body, names):
    root = parse_xml(body)
    values = {}
    for propstat in root.iter(f"{{{DAV}}}propstat"):
        if " 200 " not in (propstat.findtext(f"{{{DAV}}}status") or ""):
            continue
        for name in names:
            node = propstat.find(f"{{{DAV}}}prop/{{{namespace(name)}}}{name}")
            if node is not None:
                values[name] = (node.text or "").strip()
    return values


def expand_query():
    return (f'<C:calendar-query xmlns:D="{DAV}" xmlns:C="{CALDAV}"><D:prop>'
            '<C:calendar-data><C:expand start="20300310T000000Z" end="20300320T000000Z"/>'
            '</C:calendar-data></D:prop><C:filter><C:comp-filter name="VCALENDAR">'
            '<C:comp-filter name="VEVENT"><C:time-range start="20300310T000000Z" '
            'end="20300320T000000Z"/></C:comp-filter></C:comp-filter></C:filter>'
            '</C:calendar-query>')


def calendar_data(body):
    root = parse_xml(body)
    return [node.text or "" for node in root.iter(f"{{{CALDAV}}}calendar-data")]


class Run:
    """One gate run: its state directory, its server, its sanitized logs."""

    def __init__(self, binary, log_dir, state_dir=None, timeout=DEFAULT_TIMEOUT):
        self.binary = binary
        self.log_dir = log_dir
        self.state_dir = state_dir
        self.timeout = timeout
        self.lines = []
        self.redactor = runner.Redactor()

    def log(self, line):
        line = self.redactor.sanitize(line)
        self.lines.append(line)
        print("fresh store: " + line, flush=True)

    def run(self):
        logs = runner.log_directory(self.log_dir)
        directory, owned = None, False
        server = None
        failed = True
        previous = signal.signal(signal.SIGALRM, self.expired)
        signal.setitimer(signal.ITIMER_REAL, self.timeout)
        try:
            binary = Path(self.binary)
            if not binary.is_file() or not os.access(binary, os.X_OK):
                raise GateError("the server binary does not exist or is not executable")
            directory, owned = self.allocate()
            server = FreshServer(binary, directory)
            self.redactor = server.redactor
            gate = Gate(server, self.log)
            self.step("bootstrap the fresh store", server.bootstrap)
            self.step("create the accounts and seed data", gate.create)
            pins = self.step("record the pins", gate.snapshot_pins)
            shares = self.step("record the shares", gate.snapshot_shares)
            self.step("wait for work to drain", gate.wait_for_drain)
            self.step("stop cleanly and restart on the same store", server.restart)
            self.step("read back with new connections",
                      lambda: gate.readback(pins, shares))
            missing = [check for check in CHECKS if check not in gate.passed]
            if missing:
                raise GateError("readback did not report: " + "; ".join(missing))
            failed = False
            self.log(f"PASSED: {len(CHECKS)} readback checks after the restart")
        except (GateError, GateTimeout) as error:
            self.log("FAILED: " + str(error))
        except (OSError, subprocess.SubprocessError, ValueError, KeyError) as error:
            self.log(f"FAILED: {type(error).__name__} in the harness")
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, previous)
            if server is not None:
                try:
                    server.close()
                except (RuntimeError, OSError, subprocess.SubprocessError):
                    failed = True
                    self.log("FAILED: the server could not be stopped")
            self.retain(logs, directory)
            os.close(logs)
            if owned and directory is not None:
                shutil.rmtree(directory, ignore_errors=True)
        return 1 if failed else 0

    def step(self, name, function):
        self.log("step: " + name)
        try:
            return function()
        except GateError:
            raise
        except RuntimeError as error:
            # runner's errors name the API and status only, never a body.
            raise GateError(f"{name}: {error}") from None

    def expired(self, signum, frame):
        raise GateTimeout(f"timed out after {self.timeout} seconds")

    def allocate(self):
        if self.state_dir is None:
            directory = Path(tempfile.mkdtemp(prefix="stalwart-fresh-store-"))
            os.chmod(directory, 0o700)
            return directory, True
        directory = Path(self.state_dir)
        if os.path.lexists(directory):
            raise GateError("the state directory already exists; the gate only creates "
                            "fresh stores and has no conversion mode")
        directory.mkdir(mode=0o700)
        return directory, False

    def retain(self, logs, directory):
        server_log = ""
        if directory is not None and (directory / "server.log").is_file():
            server_log = (directory / "server.log").read_text(errors="replace")
        for name, content in (("server.log", server_log),
                              ("fresh-store.log", "\n".join(self.lines) + "\n")):
            try:
                runner.retain_log(logs, name, self.redactor.sanitize(content))
            except OSError:
                print("fresh store: could not retain " + name, file=sys.stderr)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--log-dir", required=True, type=Path,
                        help="New private directory for the sanitized logs")
    parser.add_argument("--state-dir", type=Path,
                        help="New directory for the store (default: a temporary one, "
                             "removed at the end); an existing path is refused")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT,
                        help="Seconds the whole run may take")
    args = parser.parse_args(argv)
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    try:
        return Run(args.binary, args.log_dir, args.state_dir, args.timeout).run()
    except (OSError, ValueError) as error:
        # The log directory itself could not be created: nothing ran.
        print(f"fresh store: FAILED: {type(error).__name__}: the log directory", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
