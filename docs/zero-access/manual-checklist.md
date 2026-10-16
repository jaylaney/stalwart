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
13. Invitations between two key accounts (release 2), with a client connected to each: from the first account, invite the second; confirm the invitation appears in the second client's inbox and on its calendar at its next refresh (no email is sent). Accept it from the second client; confirm the first client shows the attendee as accepted. Move the event from the first client; confirm the second client shows the new time. Delete the event from the first client; confirm the second client shows it cancelled. Repeat the invitation once with an external address alongside the second account; confirm the client shows delivery to the external address as not attempted (`SCHEDULE-STATUS` 5.3, shown as a delivery failure by clients that show it) and that nothing is sent.
14. Reply handling across clients (release 2): with an organizer client and an attendee client connected to two key accounts, invite, let the attendee accept, then move the event from the organizer and let the attendee accept again. The organizer must show the second acceptance. A client that strips SEQUENCE from the attendee's copy would have every reply to a rescheduled event kept as history (the drain's stale-reply check reads an absent SEQUENCE as 0); if the organizer never shows the second acceptance, record the client and its version.

Server-side, after the run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests` passes against the same build, and the admin
can read nothing meaningful in the stored records (spot-check with the leak scanner's output).
