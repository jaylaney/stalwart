#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Plan 3 gating checks against the running local server: scheduling and sharing are off for key accounts.
# Needs alice@example.com (the key account, after its setup flow) and plain@example.com (the ordinary
# account provision.sh creates when PLAIN_PASS is set). Their passwords come from the environment:
#   ALICE_PASS  alice's password       PLAIN_PASS  plain's password
# Both are required; put them in $ZA_RUN_DIR/local.env or export them.
# Usage: ./gatecheck3.sh
# Keeps "set -u" only (no -e/pipefail): several checks are pipelines that legitimately match nothing.
set -u
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
missing=""
for v in ALICE_PASS PLAIN_PASS; do
  if [ -z "${!v:-}" ]; then missing="$missing $v"; fi
done
if [ -n "$missing" ]; then
  echo "gatecheck3.sh: required variable(s) not set:$missing" >&2
  echo "Set ALICE_PASS (alice@example.com) and PLAIN_PASS (plain@example.com) in $ZA_RUN_DIR/local.env or the environment." >&2
  exit 1
fi
ALICE="alice@example.com:$ALICE_PASS"
PLAIN="plain@example.com:$PLAIN_PASS"
CAL="urn:ietf:params:xml:ns:caldav"
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
present() { if grep -q -F -- "$2" <<<"$1"; then echo present; else echo absent; fi; }
dav_header() { curl -s -o /dev/null -D - -X OPTIONS "$@" "$BASE/dav/cal/" | tr -d '\r' | grep -i '^dav:' || true; }
acl_body() { # $1 = principal href, empty for a cleared grant
  if [ -z "$1" ]; then
    echo '<?xml version="1.0"?><D:acl xmlns:D="DAV:"/>'
  else
    echo "<?xml version=\"1.0\"?><D:acl xmlns:D=\"DAV:\"><D:ace><D:principal><D:href>$1</D:href></D:principal><D:grant><D:privilege><D:read/></D:privilege></D:grant></D:ace></D:acl>"
  fi
}
hrefs() { grep -o '<D:response>' <<<"$1" | wc -l | tr -d ' '; }  # one response per member plus the collection
event() { # uid summary organizer attendee
  printf 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//gatecheck3//EN\r\nBEGIN:VEVENT\r\nUID:%s\r\nDTSTAMP:20990101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:%s\r\nORGANIZER:mailto:%s\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:%s\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n' "$1" "$2" "$3" "$4"
}

# 1-3: DAV header on OPTIONS
h=$(dav_header -u "$ALICE")
echo "1 OPTIONS alice: $h   (expect calendar-access: $(present "$h" calendar-access); calendar-auto-schedule: $(present "$h" calendar-auto-schedule), expect absent)"
h=$(dav_header -u "$PLAIN")
echo "2 OPTIONS plain: calendar-auto-schedule $(present "$h" calendar-auto-schedule)   (expect present)"
h=$(dav_header)
st=$(code -X OPTIONS "$BASE/dav/cal/")
echo "3 OPTIONS anonymous: status $st, calendar-auto-schedule $(present "$h" calendar-auto-schedule)   (expect 200, present)"

# 4-5: ACL
echo "4 ACL alice grants read to plain      $(code -u "$ALICE" -X ACL -H 'Content-Type: application/xml' --data "$(acl_body /dav/pal/plain@example.com/)" "$BASE/dav/cal/alice@example.com/default/")   (expect 403)"
echo "5a ACL plain grants read to alice     $(code -u "$PLAIN" -X ACL -H 'Content-Type: application/xml' --data "$(acl_body /dav/pal/alice@example.com/)" "$BASE/dav/cal/plain@example.com/default/")   (expect 200)"
echo "5b alice PROPFIND plain's calendar    $(code -u "$ALICE" -X PROPFIND -H 'Depth: 0' "$BASE/dav/cal/plain@example.com/default/")   (expect 207)"
echo "5c ACL plain clears the grant         $(code -u "$PLAIN" -X ACL -H 'Content-Type: application/xml' --data "$(acl_body '')" "$BASE/dav/cal/plain@example.com/default/")   (expect 200)"

# 6: scheduling properties
PROPS="<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\" xmlns:C=\"$CAL\"><D:prop><C:schedule-inbox-URL/><C:schedule-outbox-URL/><C:calendar-home-set/></D:prop></D:propfind>"
for who in alice plain; do
  creds="$ALICE"; [ "$who" = plain ] && creds="$PLAIN"
  out=$(curl -s -u "$creds" -X PROPFIND -H 'Depth: 0' -H 'Content-Type: application/xml' --data "$PROPS" "$BASE/dav/pal/$who@example.com/")
  # Split propstats, report each property with the status of its propstat.
  for p in schedule-inbox-URL schedule-outbox-URL calendar-home-set; do
    s=$(tr '\n' ' ' <<<"$out" | sed 's/<[A-Za-z0-9]*:propstat>/\
&/g' | grep -F -- "$p" | grep -o 'HTTP/1.1 [0-9]*' | head -1 | awk '{print $2}')
    [ "$who" = alice ] && { case $p in calendar-home-set) e=200;; *) e=404;; esac; } || e=200
    echo "6 PROPFIND /dav/pal/$who@example.com/ $p: ${s:-none}   (expect $e)"
  done
done

# 7: JMAP session
for who in alice plain; do
  creds="$ALICE"; [ "$who" = plain ] && creds="$PLAIN"
  # The top-level "capabilities" list is server-wide and static; the gate is on accountCapabilities/primaryAccounts.
  s=$(curl -s -u "$creds" "$BASE/jmap/session" | python3 -I -c 'import json,sys; s=json.load(sys.stdin); print(" ".join(sorted(set(k for a in s["accounts"].values() for k in a["accountCapabilities"]) | set(s["primaryAccounts"]))))')
  e=present; [ "$who" = alice ] && e=absent
  echo "7 JMAP session $who: urn:ietf:params:jmap:calendars $(present "$s" urn:ietf:params:jmap:calendars) in accountCapabilities/primaryAccounts   (expect $e)"
done

# 8: invite flow alice -> plain
tag=$(event invite-manual-1 invite-canary-alice alice@example.com plain@example.com | curl -s -o /dev/null -D - -u "$ALICE" -X PUT -H 'Content-Type: text/calendar' --data-binary @- "$BASE/dav/cal/alice@example.com/default/invite-manual.ics" | tr -d '\r')
echo "8a PUT invite-manual.ics as alice     $(grep -m1 '^HTTP' <<<"$tag" | awk '{print $2}')   (expect 201); Schedule-Tag: $(if grep -qi '^schedule-tag:' <<<"$tag"; then echo returned; else echo none; fi)   (expect none)"
sleep 2
out=$(curl -s -u "$PLAIN" -X PROPFIND -H 'Depth: 1' "$BASE/dav/itip/plain@example.com/inbox/")
echo "8b plain itip inbox hrefs             $(hrefs "$out")   (expect 1, the collection itself)"
echo "8c DELETE invite-manual.ics as alice  $(code -u "$ALICE" -X DELETE "$BASE/dav/cal/alice@example.com/default/invite-manual.ics")   (expect 204)"

# 9: reverse
st=$(event invite-manual-2 invite-canary-plain plain@example.com alice@example.com | curl -s -o /dev/null -w '%{http_code}' -u "$PLAIN" -X PUT -H 'Content-Type: text/calendar' --data-binary @- "$BASE/dav/cal/plain@example.com/default/invite-manual2.ics")
echo "9a PUT invite-manual2.ics as plain    $st   (expect 201)"
sleep 2
out=$(curl -s -u "$ALICE" -X PROPFIND -H 'Depth: 1' "$BASE/dav/itip/alice@example.com/inbox/")
echo "9b alice itip inbox hrefs             $(hrefs "$out")   (expect 1)"
echo "9c DELETE invite-manual2.ics as plain $(code -u "$PLAIN" -X DELETE "$BASE/dav/cal/plain@example.com/default/invite-manual2.ics")   (expect 204)"

echo "Next: $(dirname "$0")/leakcheck.sh invite-canary-alice invite-canary-plain   (alice's canary: 0 files; plain's may appear, plaintext by design)"
