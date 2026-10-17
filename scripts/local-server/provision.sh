#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Run against the recovery-mode server (start-recovery.sh, plain HTTP on 127.0.0.1:8080).
# Creates, through the registry JMAP API as the fallback admin: three listeners (http 8080 and
# imap 1143, both plain; https 8443 with TLS), the certificate from $ZA_RUN_DIR/tls (made by
# tls-cert.sh) set as the default certificate, the domain example.com, and a user
# alice@example.com with NO password credential (eligible for the zero-access setup flow).
# If PLAIN_PASS is set it also creates the ordinary account plain@example.com with that
# password (the control account of the gating checks).
# Idempotent enough to rerun: duplicates are refused.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
ADMIN="$STALWART_RECOVERY_ADMIN"
RECOVERY_BASE="http://127.0.0.1:8080"   # the recovery server; BASE (TLS) exists only after this script
USING='["urn:ietf:params:jmap:core","urn:stalwart:jmap"]'
CERT="$ZA_RUN_DIR/tls/cert.pem"
KEY="$ZA_RUN_DIR/tls/key.pem"

for f in "$CERT" "$KEY"; do
  if [ ! -f "$f" ]; then
    echo "provision.sh: $f is missing; run tls-cert.sh first." >&2
    exit 1
  fi
done

ACC=$(curl -sf -u "$ADMIN" "$RECOVERY_BASE/jmap/session" | python3 -I -c 'import json,sys; s=json.load(sys.stdin); print(next(iter(s["accounts"])))')
echo "admin account id: $ACC"

jmap_call() { # $1 method name, $2 JSON arguments after accountId; the body goes through stdin
  printf '%s' "{\"using\":$USING,\"methodCalls\":[[\"$1\",{\"accountId\":\"$ACC\",$2},\"0\"]]}" |
    curl -sf -u "$ADMIN" -H 'Content-Type: application/json' "$RECOVERY_BASE/jmap" --data-binary @-
}
jmap_create() { # $1 object type, $2 JSON object
  jmap_call "x:$1/set" "\"create\":{\"i0\":$2}"
}
created_id() { python3 -I -c 'import json,sys; r=json.load(sys.stdin)["methodResponses"][0][1]; c=r.get("created",{}).get("i0"); print(c["id"] if c else "ERROR: "+json.dumps(r.get("notCreated")))'; }
updated_ok() { python3 -I -c 'import json,sys; r=json.load(sys.stdin)["methodResponses"][0][1]; print("ok" if "singleton" in (r.get("updated") or {}) else "ERROR: "+json.dumps(r.get("notUpdated")))'; }

echo "listener http 8080: $(jmap_create NetworkListener '{"name":"http","bind":{"[::]:8080":true},"protocol":"http","useTls":false,"tlsImplicit":false}' | created_id)"
echo "listener imap 1143: $(jmap_create NetworkListener '{"name":"imap","bind":{"[::]:1143":true},"protocol":"imap","useTls":false,"tlsImplicit":false}' | created_id)"
echo "listener https 8443: $(jmap_create NetworkListener '{"name":"https","bind":{"[::]:8443":true},"protocol":"http","useTls":true,"tlsImplicit":true}' | created_id)"

# issuer and subjectAlternativeNames are server-set and refused on create.
CERT_JSON=$(python3 -I -c 'import json,sys; print(json.dumps({"certificate":{"@type":"File","filePath":sys.argv[1]},"privateKey":{"@type":"File","filePath":sys.argv[2]}},separators=(",",":")))' "$CERT" "$KEY")
CERT_ID=$(jmap_create Certificate "$CERT_JSON" | created_id)
echo "certificate $CERT: $CERT_ID"
case "$CERT_ID" in
  ERROR:*) echo "default certificate: skipped (no certificate id)" ;;
  *) echo "default certificate: $(jmap_call 'x:SystemSettings/set' "\"update\":{\"singleton\":{\"defaultCertificateId\":\"$CERT_ID\"}}" | updated_ok)" ;;
esac

DOMAIN_ID=$(jmap_create Domain '{"name":"example.com","isEnabled":true,"description":"Zero-access test domain"}' | created_id)
echo "domain example.com: $DOMAIN_ID"
USER_JSON="{\"@type\":\"User\",\"name\":\"alice\",\"domainId\":\"$DOMAIN_ID\",\"credentials\":{},\"description\":\"Alice (zero-access test)\"}"
echo "user alice@example.com (no password): $(jmap_create Account "$USER_JSON" | created_id)"

if [ -n "${PLAIN_PASS:-}" ]; then
  PLAIN_JSON=$(printf '%s' "$PLAIN_PASS" | python3 -I -c 'import json,sys; print(json.dumps({"@type":"User","name":"plain","domainId":sys.argv[1],"credentials":{"0":{"@type":"Password","secret":sys.stdin.read()}},"description":"Plain (ordinary account, gating control)"},separators=(",",":")))' "$DOMAIN_ID")
  echo "user plain@example.com (ordinary, password from PLAIN_PASS): $(jmap_create Account "$PLAIN_JSON" | created_id)"
else
  echo "user plain@example.com: skipped (PLAIN_PASS not set)"
fi

echo
echo "Done. Stop the recovery server (Ctrl-C in its terminal), then run $(dirname "$0")/start.sh."
