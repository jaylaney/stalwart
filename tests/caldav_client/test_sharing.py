# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Mandatory sharing contracts through each actor's real python-caldav client."""
import json
import time
from urllib import error as url_error
from urllib import parse, request
import uuid
import xml.etree.ElementTree as ET

from caldav.lib import error
from caldav.lib.python_utilities import to_wire
from caldav.response import DAVResponse
import pytest

from test_flows import attendee, event_data, partstat, people


DAV = "DAV:"
APPLE = "http://apple.com/ns/ical/"


def api(user, endpoint, **fields):
    """Use a user's primary-password public API; never include bodies in failures."""
    selected = {"contacts/lookup": ("fingerprint",), "contacts/pin": ("ok",),
                "shares/grant": ("ok", "changed"), "shares/revoke": ("ok", "changed")}
    if endpoint not in selected:
        raise AssertionError("unsupported public API endpoint") from None
    origin = parse.urlsplit(user["url"])
    url = f"{origin.scheme}://{origin.netloc}/api/vault/{endpoint}"
    body = json.dumps({"username": user["username"], "password": user["password"],
                       **fields}).encode()
    operation = request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        with request.build_opener(request.ProxyHandler({})).open(operation, timeout=10) as reply:
            status = reply.status
            try:
                data = json.load(reply)
            except (UnicodeError, ValueError):
                raise AssertionError(f"public API {endpoint} returned invalid JSON") from None
    except url_error.HTTPError as failure:
        raise AssertionError(f"public API {endpoint} returned HTTP {failure.code}") from None
    except url_error.URLError:
        raise AssertionError(f"public API {endpoint} was unavailable") from None
    if status != 200 or not isinstance(data, dict):
        raise AssertionError(f"public API {endpoint} returned an invalid response") from None
    return {field: data.get(field) for field in selected[endpoint]}


def pin(owner, recipient):
    found = api(owner, "contacts/lookup", address=recipient["username"])
    fingerprint = found.get("fingerprint")
    if not isinstance(fingerprint, str) or not fingerprint:
        raise AssertionError("contact lookup returned an invalid fingerprint") from None
    pinned = api(owner, "contacts/pin", address=recipient["username"],
                 fingerprint=fingerprint)
    if pinned.get("ok") is not True:
        raise AssertionError("contact pin did not succeed") from None


def grant(owner, slug, recipient, level):
    changed = api(owner, "shares/grant", calendar=slug, address=recipient["username"],
                  level=level)
    if changed.get("ok") is not True or changed.get("changed") is not True:
        raise AssertionError("calendar grant did not change access") from None


def revoke(owner, slug, recipient):
    changed = api(owner, "shares/revoke", calendar=slug, address=recipient["username"])
    if changed.get("ok") is not True or changed.get("changed") is not True:
        raise AssertionError("calendar revoke did not change access") from None


def path_of(calendar):
    return parse.unquote(parse.urlsplit(str(calendar.url)).path)


def actor_calendar(principal, url):
    target = parse.unquote(parse.urlsplit(str(url)).path)
    matches = [calendar for calendar in principal.calendars()
               if path_of(calendar) == target]
    assert len(matches) == 1, "granted calendar was absent or duplicated in discovery"
    return matches[0]


def own_default(principal, username):
    home = f"/dav/cal/{username}/"
    matches = [calendar for calendar in principal.calendars()
               if path_of(calendar) == home + "default/"]
    assert len(matches) == 1, "recipient own default calendar was absent or duplicated"
    return matches[0]


def poll_state(client, read, condition, label):
    """Accept a sharing read only inside 15 seconds, restoring the client's timeout."""
    deadline = time.monotonic() + 15
    previous_timeout = client.timeout
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                pytest.fail(f"{label} did not reach the required state within 15s")
            # A finite transport inactivity timeout bounds each HTTP request. The
            # monotonic checks enforce the outcome deadline even if a read finishes late.
            client.timeout = min(previous_timeout, remaining) if (
                isinstance(previous_timeout, (int, float)) and previous_timeout > 0
            ) else remaining
            try:
                value = read()
            except error.NotFoundError:
                value = None
            if time.monotonic() >= deadline:
                pytest.fail(f"{label} did not reach the required state within 15s")
            if value is not None and condition(value):
                if time.monotonic() >= deadline:
                    pytest.fail(f"{label} did not reach the required state within 15s")
                return value
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                pytest.fail(f"{label} did not reach the required state within 15s")
            time.sleep(min(0.1, remaining))
    finally:
        client.timeout = previous_timeout


def poll_owned(principal, username, uid, condition=lambda event: True):
    # A principal-wide UID search can return Alice's shared source instead of Bob's own copy.
    def read_owned():
        calendar = own_default(principal, username)
        event = calendar.event_by_uid(uid)
        event.load()
        if not path_of(event).startswith(path_of(calendar)):
            pytest.fail("recipient event was outside the own calendar")
        return event

    return poll_state(principal.client, read_owned, condition, "recipient own calendar")


def dav(client, url, method="GET", body="", headers=None):
    # DAVClient.request folds 401 and 403 into one AuthorizationError. Its own
    # configured session retains the exact wire status for denial assertions.
    assert client.auth is not None, "CalDAV client has not authenticated"
    target, prepared = client._prepare_request(str(url), method, body, headers)
    response = client.session.request(method, str(target), data=to_wire(body),
                                      headers=prepared, auth=client.auth,
                                      timeout=client.timeout, verify=client.ssl_verify_cert,
                                      cert=client.ssl_cert)
    return response.status_code, DAVResponse(response, client)


def properties(client, url):
    body = (f'<D:propfind xmlns:D="{DAV}" xmlns:A="{APPLE}"><D:prop>'
            '<D:displayname/><A:calendar-color/><D:getetag/></D:prop></D:propfind>')
    status, reply = dav(client, url, "PROPFIND", body, {"Depth": "0"})
    assert status == 207
    root = ET.fromstring(reply._raw)
    return {tag: root.findtext(f".//{{{namespace}}}{tag}")
            for namespace, tag in ((DAV, "displayname"), (APPLE, "calendar-color"),
                                   (DAV, "getetag"))}


def property_patch(tag, value):
    prefix = "D" if tag == "displayname" else "A"
    return (f'<D:propertyupdate xmlns:D="{DAV}" xmlns:A="{APPLE}"><D:set><D:prop>'
            f'<{prefix}:{tag}>{value}</{prefix}:{tag}>'
            '</D:prop></D:set></D:propertyupdate>')


def sync(client, url, token=""):
    body = (f'<D:sync-collection xmlns:D="{DAV}"><D:sync-token>{token}</D:sync-token>'
            '<D:sync-level>1</D:sync-level><D:prop><D:getetag/></D:prop>'
            '</D:sync-collection>')
    status, reply = dav(client, url, "REPORT", body, {"Depth": "1"})
    if status != 207:
        return status, None, set()
    root = ET.fromstring(reply._raw)
    next_token = root.findtext(f".//{{{DAV}}}sync-token")
    hrefs = {node.text for node in root.findall(f".//{{{DAV}}}response/{{{DAV}}}href")}
    return status, next_token, hrefs


def shared_fixture(people):
    users, principals = people
    slug = "sharing-" + uuid.uuid4().hex
    calendar = principals[0].make_calendar(name="Owner shared calendar", cal_id=slug)
    pin(users[0], users[1])
    return users, principals, slug, calendar


def test_shared_calendar_view_and_edit(people):
    users, principals, slug, calendar = shared_fixture(people)
    private = principals[0].make_calendar(
        name="Owner private", cal_id="private-" + uuid.uuid4().hex)
    try:
        uid = uuid.uuid4().hex
        calendar.add_event(event_data(uid, "Owner event").to_ical())
        grant(users[0], slug, users[1], "view")
        bob = actor_calendar(principals[1], calendar.url)
        assert str(bob.event_by_uid(uid).icalendar_component["SUMMARY"]) == "Owner event"
        assert all(path_of(item) != path_of(private) for item in principals[1].calendars())
        status, _ = dav(principals[1].client, str(bob.url) + "denied.ics", "PUT",
                        event_data(uuid.uuid4().hex).to_ical().decode(),
                        {"Content-Type": "text/calendar"})
        assert status == 403
        grant(users[0], slug, users[1], "edit")
        bob = actor_calendar(principals[1], calendar.url)
        new_uid = uuid.uuid4().hex
        created = bob.add_event(event_data(new_uid, "Bob created").to_ical())
        assert str(calendar.event_by_uid(new_uid).icalendar_component["SUMMARY"]) == "Bob created"
        created.icalendar_component["SUMMARY"] = "Bob edited"
        created.save()
        assert str(calendar.event_by_uid(new_uid).icalendar_component["SUMMARY"]) == "Bob edited"
        created.delete()
        with pytest.raises(error.NotFoundError):
            calendar.event_by_uid(new_uid)
    finally:
        private.delete()
        calendar.delete()


def test_shared_calendar_properties_and_etags(people):
    users, principals, slug, calendar = shared_fixture(people)
    try:
        grant(users[0], slug, users[1], "edit")
        bob = actor_calendar(principals[1], calendar.url)
        patch = (f'<D:propertyupdate xmlns:D="{DAV}" xmlns:A="{APPLE}"><D:set><D:prop>'
                 '<D:displayname>Bob personal name</D:displayname>'
                 '<A:calendar-color>#123456FF</A:calendar-color>'
                 '</D:prop></D:set></D:propertyupdate>')
        assert dav(principals[1].client, bob.url, "PROPPATCH", patch)[0] == 207
        owner_properties = properties(principals[0].client, calendar.url)
        bob_properties = properties(principals[1].client, bob.url)
        assert owner_properties["displayname"] == "Owner shared calendar"
        assert bob_properties["displayname"] == "Bob personal name"
        assert owner_properties["calendar-color"] == bob_properties["calendar-color"] == "#123456FF"
        for client, url, tag, competing, stale, current, unconditional in (
            (principals[0].client, calendar.url, "displayname", "Competing owner name",
             "Stale owner name", "Current owner name", "Unconditional owner name"),
            (principals[1].client, bob.url, "calendar-color", "#222222FF",
             "#333333FF", "#444444FF", "#555555FF"),
        ):
            old_etag = properties(client, url)["getetag"]
            assert old_etag
            assert dav(client, url, "PROPPATCH", property_patch(tag, competing))[0] == 207
            assert dav(client, url, "PROPPATCH", property_patch(tag, stale),
                       {"If-Match": old_etag})[0] == 412
            assert properties(client, url)[tag] == competing
            fresh_etag = properties(client, url)["getetag"]
            assert fresh_etag and fresh_etag != old_etag
            assert dav(client, url, "PROPPATCH", property_patch(tag, current),
                       {"If-Match": fresh_etag})[0] == 207
            assert properties(client, url)[tag] == current
            assert dav(client, url, "PROPPATCH", property_patch(tag, unconditional))[0] == 207
            assert properties(client, url)[tag] == unconditional
        assert properties(principals[0].client, calendar.url)["calendar-color"] == "#555555FF"
    finally:
        calendar.delete()


def test_shared_calendar_revoke_regrant_rename_and_sync(people):
    users, principals, slug, calendar = shared_fixture(people)
    private = principals[0].make_calendar(name="Not shared", cal_id="private-" + uuid.uuid4().hex)
    current_url = str(calendar.url)
    moved = False
    try:
        uid = uuid.uuid4().hex
        calendar.add_event(event_data(uid).to_ical())
        private.add_event(event_data(uuid.uuid4().hex).to_ical())
        grant(users[0], slug, users[1], "edit")
        bob = actor_calendar(principals[1], current_url)
        control_uid = uuid.uuid4().hex
        bob.add_event(event_data(control_uid, "Pre-revoke Bob write").to_ical())
        assert str(calendar.event_by_uid(control_uid).icalendar_component["SUMMARY"]) == (
            "Pre-revoke Bob write")
        status, old_token, _ = sync(principals[1].client, bob.url)
        assert status == 207 and old_token
        revoke(users[0], slug, users[1])
        assert all(path_of(item) != path_of(calendar) for item in principals[1].calendars())
        assert dav(principals[1].client, current_url, "PROPFIND", headers={"Depth": "0"})[0] == 403
        denied_uid = uuid.uuid4().hex
        assert dav(principals[1].client, current_url + denied_uid + ".ics", "PUT",
                   event_data(denied_uid).to_ical().decode(),
                   {"Content-Type": "text/calendar"})[0] == 403
        grant(users[0], slug, users[1], "view")
        bob = actor_calendar(principals[1], current_url)
        assert bob.event_by_uid(uid)
        assert sync(principals[1].client, bob.url, old_token)[0] == 403
        post_grant_uid = uuid.uuid4().hex
        calendar.add_event(event_data(post_grant_uid, "Post-regrant owner change").to_ical())
        status, rename_token, hrefs = sync(principals[1].client, bob.url)
        assert status == 207 and rename_token
        assert any(post_grant_uid in href for href in hrefs)
        # This same token must be admitted incrementally before the rename.
        assert sync(principals[1].client, bob.url, rename_token)[0] == 207
        renamed = "renamed-" + uuid.uuid4().hex
        new_url = current_url.rsplit("/", 2)[0] + "/" + renamed + "/"
        assert dav(principals[0].client, current_url, "MOVE",
                   headers={"Destination": new_url})[0] == 201
        moved = True
        assert all(path_of(item) != parse.unquote(parse.urlsplit(current_url).path)
                   for item in principals[1].calendars())
        assert actor_calendar(principals[1], new_url).event_by_uid(uid)
        assert sync(principals[1].client, new_url, rename_token)[0] == 403
        status, token, hrefs = sync(principals[1].client, new_url)
        assert status == 207 and token
        assert any(uid in href for href in hrefs)
        assert all("/private-" not in href for href in hrefs)
    finally:
        private.delete()
        target = new_url if moved else current_url
        assert dav(principals[0].client, target, "DELETE")[0] == 204


def test_shared_calendar_delegated_invitation_update_cancel(people):
    users, principals, slug, calendar = shared_fixture(people)
    alice, bob, carol = [user["username"] for user in users]
    try:
        pin(users[0], users[2])
        pin(users[1], users[2])
        grant(users[0], slug, users[1], "edit")
        bob_shared = actor_calendar(principals[1], calendar.url)
        uid = uuid.uuid4().hex
        source = bob_shared.add_event(event_data(uid, attendees=[carol], organizer=alice).to_ical())
        invited = poll_owned(principals[2], carol, uid)
        assert str(invited.icalendar_component["ORGANIZER"]) == "mailto:" + alice
        assert str(invited.icalendar_component["ORGANIZER"].params["SENT-BY"]) == "mailto:" + bob
        assert partstat(invited, carol) == "NEEDS-ACTION"
        source.icalendar_component["SUMMARY"] = "Delegated update"
        source.icalendar_component["SEQUENCE"] = 1
        source.save()
        updated = poll_owned(principals[2], carol, uid,
                             lambda event: str(event.icalendar_component.get("SUMMARY"))
                             == "Delegated update")
        assert str(updated.icalendar_component["ORGANIZER"].params["SENT-BY"]) == "mailto:" + bob
        source.icalendar_component["STATUS"] = "CANCELLED"
        source.icalendar_component["SEQUENCE"] = 2
        source.save()
        cancelled = poll_owned(principals[2], carol, uid,
                               lambda event: str(event.icalendar_component.get("STATUS"))
                               == "CANCELLED")
        assert str(cancelled.icalendar_component["ORGANIZER"]) == "mailto:" + alice
    finally:
        calendar.delete()


def test_shared_calendar_own_and_shared_replies_and_sole_decline(people):
    users, principals, slug, calendar = shared_fixture(people)
    alice, bob = [user["username"] for user in users[:2]]
    try:
        pin(users[1], users[0])
        grant(users[0], slug, users[1], "edit")
        bob_shared = actor_calendar(principals[1], calendar.url)
        own_uid = uuid.uuid4().hex
        calendar.add_event(event_data(own_uid, attendees=[bob], organizer=alice).to_ical())
        own_copy = poll_owned(principals[1], bob, own_uid)
        assert path_of(own_copy) != path_of(calendar.event_by_uid(own_uid))
        own_copy.change_attendee_status("mailto:" + bob, PARTSTAT="ACCEPTED")
        own_copy.save()
        poll_state(principals[0].client, lambda: calendar.event_by_uid(own_uid),
                   lambda event: partstat(event, bob) == "ACCEPTED",
                   "own-copy reply source")
        shared_uid = uuid.uuid4().hex
        calendar.add_event(event_data(shared_uid, attendees=[bob], organizer=alice).to_ical())
        invitation = poll_owned(principals[1], bob, shared_uid)
        assert path_of(invitation) != path_of(bob_shared.event_by_uid(shared_uid))
        source = bob_shared.event_by_uid(shared_uid)
        source.change_attendee_status("mailto:" + bob, PARTSTAT="DECLINED")
        source.save()
        declined = poll_owned(principals[1], bob, shared_uid,
                              lambda event: partstat(event, bob) == "DECLINED")
        assert partstat(declined, bob) == "DECLINED"
        assert partstat(calendar.event_by_uid(shared_uid), bob) == "DECLINED"
        assert attendee(declined, bob).params.get("SCHEDULE-FORCE-SEND") is None
    finally:
        calendar.delete()
