# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Strict client-level assertions that the upstream auto-scheduling path omits."""
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import time
import uuid

import caldav
from caldav.lib import error
import icalendar
import pytest


@pytest.fixture
def people():
    users = json.loads(Path(os.environ["CALDAV_CI_USERS"]).read_text())
    clients = [caldav.DAVClient(**user) for user in users]
    try:
        principals = [client.principal() for client in clients]
        assert all(principal.calendar_user_address_set() for principal in principals)
        yield users, principals
    finally:
        for client in clients:
            client.close()


def event_data(uid, summary="Client interoperability", attendees=(),
               organizer=None, recurrence=False):
    event = icalendar.Event()
    event.add("uid", uid)
    event.add("dtstamp", datetime.now(timezone.utc))
    event.add("dtstart", datetime(2030, 3, 11, 14, tzinfo=timezone.utc))
    event.add("dtend", datetime(2030, 3, 11, 15, tzinfo=timezone.utc))
    event.add("summary", summary)
    event.add("sequence", 0)
    if recurrence:
        event.add("rrule", {"freq": "daily", "count": 3})
    if organizer:
        event.add("organizer", "mailto:" + organizer)
    for attendee in attendees:
        address = icalendar.vCalAddress("mailto:" + attendee)
        address.params.update({"PARTSTAT": "NEEDS-ACTION", "RSVP": "TRUE"})
        event.add("attendee", address)
    calendar = icalendar.Calendar()
    calendar.add("prodid", "-//Stalwart fork//python-caldav CI//EN")
    calendar.add("version", "2.0")
    calendar.add_component(event)
    return calendar


def poll_event(principal, uid, condition=lambda event: True, timeout=15):
    # Stay below the 30-second drain retry deferral: an initial failure must be visible.
    deadline = time.monotonic() + timeout
    while True:
        # Every authenticated DAV read triggers the key-account drain.
        for calendar in principal.calendars():
            try:
                event = calendar.event_by_uid(uid)
                event.load()
            except error.NotFoundError:
                continue
            if condition(event):
                return event
        if time.monotonic() >= deadline:
            pytest.fail(f"event {uid} did not reach required state within {timeout}s")
        time.sleep(0.1)


def attendee(event, email):
    lines = event.icalendar_component.get("attendee", [])
    if not isinstance(lines, list):
        lines = [lines]
    return next((line for line in lines if str(line).lower() == "mailto:" + email.lower()), None)


def partstat(event, email):
    line = attendee(event, email)
    return str(line.params.get("PARTSTAT", "")) if line is not None else None


def test_discovery_event_and_recurrence_override(people):
    _, principals = people
    calendar = principals[0].make_calendar(
        name="Strict client flow", cal_id="strict-" + uuid.uuid4().hex)
    try:
        uid = uuid.uuid4().hex
        resource = calendar.add_event(event_data(uid, recurrence=True).to_ical())
        loaded = calendar.event_by_uid(uid)
        assert str(loaded.icalendar_component["SUMMARY"]) == "Client interoperability"
        assert str(loaded.icalendar_component["UID"]) == uid
        data = loaded.icalendar_instance
        override = icalendar.Event()
        for key, value in event_data(uid, "Moved occurrence").subcomponents[0].items():
            override.add(key, value)
        override["DTSTART"] = icalendar.vDDDTypes(datetime(2030, 3, 12, 16, tzinfo=timezone.utc))
        override["DTEND"] = icalendar.vDDDTypes(datetime(2030, 3, 12, 17, tzinfo=timezone.utc))
        override.add("recurrence-id", datetime(2030, 3, 12, 14, tzinfo=timezone.utc))
        data.add_component(override)
        resource.data = data.to_ical()
        resource.save()
        reread = calendar.event_by_uid(uid)
        components = reread.icalendar_instance.walk("VEVENT")
        assert len(components) == 2
        expanded = calendar.search(start=datetime(2030, 3, 11, tzinfo=timezone.utc),
                                   end=datetime(2030, 3, 14, tzinfo=timezone.utc),
                                   event=True, expand=True)
        starts = sorted(event.icalendar_component.decoded("DTSTART") for event in expanded)
        assert starts == [datetime(2030, 3, day, hour, tzinfo=timezone.utc)
                          for day, hour in ((11, 14), (12, 16), (13, 14))]
    finally:
        calendar.delete()


def test_invitation_acceptance_and_cancellation(people):
    users, principals = people
    alice, bob = [user["username"] for user in users[:2]]
    calendar = principals[0].calendars()[0]
    uid = uuid.uuid4().hex
    organizer = calendar.add_event(event_data(uid, attendees=[bob], organizer=alice).to_ical())
    try:
        invited = poll_event(principals[1], uid)
        assert partstat(invited, bob) == "NEEDS-ACTION"
        # The inbox is a readable scheduling collection; key-mode import drains it.
        history = [item for item in principals[1].schedule_inbox().get_items() if item.id == uid]
        assert history and any(item.is_invite_request() for item in history)
        invited.change_attendee_status("mailto:" + bob, PARTSTAT="ACCEPTED")
        invited.save()
        poll_event(principals[0], uid, lambda event: partstat(event, bob) == "ACCEPTED")
        organizer.load()
        organizer.icalendar_component["STATUS"] = "CANCELLED"
        organizer.save()
        cancelled = poll_event(principals[1], uid,
                               lambda event: str(event.icalendar_component.get("STATUS"))
                               == "CANCELLED")
        assert str(cancelled.icalendar_component["STATUS"]) == "CANCELLED"
    finally:
        organizer.delete()


def test_delegation_and_organizer_mediated_status(people):
    users, principals = people
    alice, bob, carol = [user["username"] for user in users]
    uid = uuid.uuid4().hex
    organizer = principals[0].calendars()[0].add_event(
        event_data(uid, attendees=[bob], organizer=alice).to_ical())
    try:
        delegated = poll_event(principals[1], uid)
        delegated.change_attendee_status("mailto:" + bob, PARTSTAT="DELEGATED",
                                        **{"DELEGATED-TO": "mailto:" + carol})
        delegate = icalendar.vCalAddress("mailto:" + carol)
        delegate.params.update({"PARTSTAT": "NEEDS-ACTION", "DELEGATED-FROM": "mailto:" + bob})
        delegated.icalendar_component.add("attendee", delegate)
        delegated.save()
        if os.environ["CALDAV_CI_MODE"] == "key":
            for calendar in principals[2].calendars():
                with pytest.raises(error.NotFoundError):
                    calendar.event_by_uid(uid)
            assert not [item for item in principals[2].schedule_inbox().get_items()
                        if item.id == uid]
        mediated = poll_event(principals[0], uid, lambda event: partstat(event, bob) == "DELEGATED")
        assert str(attendee(mediated, bob).params["DELEGATED-TO"]) == "mailto:" + carol
        invited = poll_event(principals[2], uid)
        assert str(attendee(invited, carol).params["DELEGATED-FROM"]) == "mailto:" + bob
        bob_history = {str(item.url) for item in principals[1].schedule_inbox().get_items()
                       if item.id == uid}
        invited.change_attendee_status("mailto:" + carol, PARTSTAT="ACCEPTED")
        invited.save()
        poll_event(principals[0], uid, lambda event: partstat(event, carol) == "ACCEPTED")
        if os.environ["CALDAV_CI_MODE"] == "key":
            before_update = poll_event(principals[1], uid)
            assert partstat(before_update, carol) == "NEEDS-ACTION"
            after_history = {str(item.url)
                             for item in principals[1].schedule_inbox().get_items()
                             if item.id == uid}
            assert after_history == bob_history
        organizer.load()
        organizer.icalendar_component["SUMMARY"] = "Organizer forwarded delegate acceptance"
        organizer.icalendar_component["SEQUENCE"] = (
            int(organizer.icalendar_component["SEQUENCE"]) + 1)
        organizer.save()
        poll_event(principals[1], uid, lambda event: partstat(event, carol) == "ACCEPTED")
    finally:
        organizer.delete()
