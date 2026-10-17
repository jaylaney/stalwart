#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# End-to-end zero-access smoke test against the running normal-mode server (start.sh).
# Usage: ./smoke.sh [localpart] [password]   (default localpart: bob; domain example.com)
# The password is the second argument, else SMOKE_PASS, else a random one that is not shown
# (the account is for this run only). Passwords and recovery keys are never printed.
# Creates a passwordless user, issues a setup token as admin, runs setup, then checks CalDAV,
# app passwords, password change, recovery-key reissue and the registry credential refusal.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
ADMIN="$STALWART_RECOVERY_ADMIN"
LOCAL="${1:-bob}"; PASS="${2:-${SMOKE_PASS:-}}"; USER="$LOCAL@example.com"
if [ -z "$PASS" ]; then PASS="$(openssl rand -hex 16)"; fi
USING='["urn:ietf:params:jmap:core","urn:stalwart:jmap"]'
J='Content-Type: application/json'
py() { python3 -I -c "$1"; }

ACC=$(curl -sf -u "$ADMIN" "$BASE/jmap/session" | py 'import json,sys; print(next(iter(json.load(sys.stdin)["accounts"])))')
DOMAIN_ID=$(curl -sf -u "$ADMIN" -H "$J" "$BASE/jmap" -d "{\"using\":$USING,\"methodCalls\":[[\"x:Domain/query\",{\"accountId\":\"$ACC\",\"filter\":{\"name\":\"example.com\"}},\"0\"]]}" | py 'import json,sys; print(json.load(sys.stdin)["methodResponses"][0][1]["ids"][0])')
USER_JSON="{\"@type\":\"User\",\"name\":\"$LOCAL\",\"domainId\":\"$DOMAIN_ID\",\"credentials\":{},\"description\":\"$LOCAL (zero-access smoke)\"}"
USER_ID=$(curl -sf -u "$ADMIN" -H "$J" "$BASE/jmap" -d "{\"using\":$USING,\"methodCalls\":[[\"x:Account/set\",{\"accountId\":\"$ACC\",\"create\":{\"i0\":$USER_JSON}},\"0\"]]}" | py 'import json,sys; r=json.load(sys.stdin)["methodResponses"][0][1]; c=r.get("created",{}).get("i0"); print(c["id"]) if c else sys.exit("create failed: "+json.dumps(r))')
echo "user $USER created, id $USER_ID"

TOKEN=$(curl -sf -u "$ADMIN" -H "$J" -X POST "$BASE/api/vault/setup-token" -d "{\"account\":\"$USER\"}" | py 'import json,sys; print(json.load(sys.stdin)["token"])')
echo "setup token issued"
RK=$(curl -sf -H "$J" -X POST "$BASE/api/vault/setup" -d "{\"username\":\"$USER\",\"token\":\"$TOKEN\",\"password\":\"$PASS\"}" | py 'import json,sys; print(json.load(sys.stdin)["recovery_key"])')
echo "setup done, recovery key issued"

dav() { curl -s -o /dev/null -w "%{http_code}" -u "$USER:$1" -X PROPFIND -H 'Depth: 0' "$BASE/dav/cal/$USER/"; }
echo "CalDAV with password:        $(dav "$PASS") (expect 207)"
echo "CalDAV with wrong password:  $(dav wrong) (expect 401)"

APP=$(curl -sf -H "$J" -X POST "$BASE/api/vault/app-password" -d "{\"username\":\"$USER\",\"password\":\"$PASS\",\"description\":\"smoke\"}" | py 'import json,sys; print(json.load(sys.stdin)["app_password"])')
echo "CalDAV with app password:    $(dav "$APP") (expect 207)"

NEWPASS="$PASS-new"
curl -sf -H "$J" -X POST "$BASE/api/vault/password" -d "{\"username\":\"$USER\",\"password\":\"$PASS\",\"new_password\":\"$NEWPASS\"}" >/dev/null
echo "CalDAV with new password:    $(dav "$NEWPASS") (expect 207)"
echo "CalDAV with old password:    $(dav "$PASS") (expect 401)"
echo "CalDAV app pw after change:  $(dav "$APP") (expect 207)"

RK2=$(curl -sf -H "$J" -X POST "$BASE/api/vault/recover" -d "{\"username\":\"$USER\",\"recovery_key\":\"$RK\",\"new_password\":\"$PASS\"}" | py 'import json,sys; print(json.load(sys.stdin)["recovery_key"])')
echo "recover with recovery key:   new recovery key issued; CalDAV with restored password: $(dav "$PASS") (expect 207)"

REFUSED=$(curl -sf -u "$ADMIN" -H "$J" "$BASE/jmap" -d "{\"using\":$USING,\"methodCalls\":[[\"x:Account/set\",{\"accountId\":\"$ACC\",\"update\":{\"$USER_ID\":{\"credentials\":{\"0\":{\"@type\":\"Password\",\"secret\":\"$PASS-registry\"}}}}},\"0\"]]}" | py 'import json,sys; r=json.load(sys.stdin)["methodResponses"][0][1]; print("refused" if "notUpdated" in r else "NOT refused: "+json.dumps(r))')
echo "registry password edit:      $REFUSED (expect refused)"
