#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Try to reach a key account's calendar without its keys. Every line prints the HTTP status.
# Usage: ./gatecheck.sh [key-account address]   (default alice@example.com)
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
ADMIN="$STALWART_RECOVERY_ADMIN"
USER="${1:-alice@example.com}"
QUERY='<?xml version="1.0"?><C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav"><D:prop><C:calendar-data/></D:prop><C:filter><C:comp-filter name="VCALENDAR"/></C:filter></C:calendar-query>'
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }

echo "admin PROPFIND  /dav/cal/$USER/          $(code -u "$ADMIN" -X PROPFIND -H 'Depth: 1' "$BASE/dav/cal/$USER/")   (expect 403)"
echo "admin REPORT    /dav/cal/$USER/default/  $(code -u "$ADMIN" -X REPORT -H 'Content-Type: application/xml' --data "$QUERY" "$BASE/dav/cal/$USER/default/")   (expect 403)"
echo "admin REPORT    /dav/pal/$USER/default/  $(code -u "$ADMIN" -X REPORT -H 'Content-Type: application/xml' --data "$QUERY" "$BASE/dav/pal/$USER/default/")   (expect 405)"
echo "admin PROPFIND  /dav/card/$USER/         $(code -u "$ADMIN" -X PROPFIND -H 'Depth: 1' "$BASE/dav/card/$USER/")   (expect 207: address books are not gated in release 1)"
echo "master login    $USER%admin PROPFIND      $(code -u "$USER%$ADMIN" -X PROPFIND -H 'Depth: 1' "$BASE/dav/cal/$USER/")   (expect 403)"
