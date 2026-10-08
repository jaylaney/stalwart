# Zero-access calendar: manual client checklist

Run once per release against a server built from this branch, with TOTP
enrolled on one of the two accounts used.

Clients: Apple Calendar on macOS and on iOS, Thunderbird, DAVx5.

For each client:
1. Add the account with the primary password (TOTP-enabled account: with an app password instead).
2. Create an event with title, location, notes, URL and a 10-minute alert; confirm it appears after a refresh.
3. Edit the title and move the event to a different calendar.
4. Create a weekly recurring event, then change one occurrence; confirm the exception survives a refresh.
5. Delete an event.
6. Rename a calendar and change its colour; confirm both after a refresh.
7. Set a custom timezone on a calendar (Thunderbird: calendar properties).
8. Copy a calendar (clients that support it) or export and re-import.
9. Go offline, edit two events, come back online; confirm both edits sync.
10. Change the password through the account page, reconnect the client with the new password.
11. Log in with an app password; revoke it through the account page; confirm the client is refused.
12. Enrol TOTP through the account page (the account page is where TOTP is enrolled and removed); confirm the client needs an app password; remove TOTP. A client with a cached session keeps working until its next authentication (spec 5), so reconnect the client before judging the result.
13. Invite an attendee from the key account; confirm the client reports that the server does not send invitations (no auto-schedule) and that nothing is delivered to the attendee.

Server-side, after the run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests` passes against the same build, and the admin
can read nothing meaningful in the stored records (spot-check with the leak scanner's output).
