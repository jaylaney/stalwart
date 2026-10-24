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
15. Key pinning's refusal (`SCHEDULE-STATUS` 3.8): with clients connected to two key accounts, invite the second account from the first, then give the second account a new key (destroy it, re-create it with the same address and set it up again; only a fresh setup changes a key), and edit the event from the first client. Record how Apple Calendar, Thunderbird and DAVx5 show `3.8` on that attendee (shown as such, shown as a delivery failure, or not shown) and their versions. The account page's contacts screen is where the user compares and confirms the new key.

Server-side, after the run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests` passes against the same build, and the admin
can read nothing meaningful in the stored records (spot-check with the leak scanner's output).

## Calendar sharing (release 4)

Use two disposable, setup-complete key accounts, an owner and grantee, with app passwords for Apple Calendar. Record OS/client versions and server build. Turn `enableAssistedDiscovery` on for Apple checks. For each observation record what the client shows immediately, then what owner and grantee show after drain and refresh; those are distinct results.

1. Grant `view` using the vault API after confirming the grantee's pin. On Apple Calendar for macOS and iOS, confirm the owner's calendar appears under the grantee account, its events read, and the client treats it as read-only; attempt a write and capture the 403/UI response.
2. Change the grant to `edit`. Edit from each Apple platform and confirm owner and grantee round trips. Change colour/order as grantee: they are shared calendar dead properties, so confirm the owner sees the colour change. Downgrade to `view`, attempt colour change, and record whether Apple keeps a local colour, reverts it, or reports the 403.
3. From the shared calendar, have the grantee invite someone and change/cancel the event. Record whether Apple offers invitees, how the recipient displays the owner's invitation and `SENT-BY` grantee, and the final recipient status. Have the grantee reply from its own invitation copy and, separately, from the shared copy; after drain and refresh, check both copies. Repeat with the grantee as sole attendee declining from the shared copy without `SCHEDULE-FORCE-SEND`; its own copy should end DECLINED. Record immediate UI separately.
4. Add a display alarm to the owner's scheduled event from the edit grantee. Drag an event in both directions between shared and grantee-owned calendars, and between owner shared and unshared calendars. Record the actual requests, response and resulting copies rather than assuming the client sends `MOVE`.
5. Revoke by the grant id listed by `shares`. Confirm the grantee loses it on the next refresh; grant again and confirm it reappears. Rename the owner's calendar and confirm discovery/sync after the boundary. Compare initial sync and incremental sync behavior where the client exposes it.
6. Turn assisted discovery off and confirm Thunderbird finds the shared calendar through the second `calendar-home-set` href. DAVx5 is optional. A recipient who is also a grantee should answer an invitation from their own copy.

Native status for this outcome: **pending** for macOS Apple Calendar, iOS Apple Calendar and Thunderbird. No native-client session was exercised in Task 12; qualified disposable native sessions and trusted-TLS setup remain pending. This checklist is not a completed native acceptance record. Record any unavailable device, credentials, certificate, network reachability or missing observation as pending.

## Mail

Release 3 (mail encryption at rest; spec `docs/superpowers/specs/2026-10-20-zero-access-mail-design.md`, section 11). Run once per release against a server built from this branch, with TOTP enrolled on one of the two key accounts used.

Clients: Apple Mail on macOS and on iOS, Thunderbird, and an Android client (FairEmail or Thunderbird for Android).

For each client:
1. Add the account with the password (TOTP-enabled account: with an app password instead), then add it again with an app password. A client that offers OAuth or a master-user login must be refused with `Zero-access accounts log in to mail with the password or an app password.`, not with a bare wrong-password error; record how each client shows the refusal.
2. On the TOTP account, confirm the password alone is refused and an app password works (the mail protocols carry no TOTP code).
3. Receive a message from an outside sender (plain text, HTML, and one with an attachment); confirm each reads in the client, the attachment included.
4. Reply to a message; confirm the reply is delivered and a copy appears in Sent.
5. Forward a message with an attachment; confirm the recipient gets the attachment intact.
6. Move a message to another folder, flag it, and delete it (expunge or empty the Trash).
7. Create a folder and rename it.
8. Search on the server for a word in a subject and in a body (observe the refusal, `Mail in this account is encrypted at rest; the server cannot search message content. Search your local copy instead.`, or an empty server result, and record which each client shows), then confirm the client's local search over its downloaded messages finds the message. A search on flags, size or date still works on the server.
9. IDLE push: leave the client open on the inbox and send the account a message from outside; confirm it appears without a manual refresh.
10. Change the password through the account page; a connected client keeps running until its session closes (mail spec 4), so reconnect it with the new password and confirm mail reads again. Revoke an app password and confirm the client using it is refused at its next connection.
11. Turn the vacation response on (through the client's vacation or ManageSieve setting, or JMAP `VacationResponse`), send the account a message from outside and confirm the sender receives the auto-reply and the message still reads in the client; turn it off and confirm no reply follows.

Server-side, after the run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests` passes against the same build, as do `imap::imap_tests` and `jmap::jmap_tests` with `ZA_KEY_ACCOUNTS=1`, and the `Mail leak scan` line of that `za_tests` run (an automated check on the test server, not on the server exercised by hand) shows only stubs for the key accounts' messages.
