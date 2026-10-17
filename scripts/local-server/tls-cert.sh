#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Generates the self-signed certificate and key for the local server's TLS listener in
# $ZA_RUN_DIR/tls/ (cert.pem, key.pem; the key is mode 0600).
# Usage: ./tls-cert.sh [--force]
#   --force overwrites existing files; without it the script refuses to.
# Environment (read from the environment only: this script runs before env.sh and local.env
# are usable, so it does not source them):
#   ZA_RUN_DIR         state directory (default: .run/ in the repository)
#   ZA_TLS_EXTRA_SAN   comma-separated extra DNS names or IP addresses for the certificate,
#                      on top of localhost, 127.0.0.1 and ::1 (e.g. "mybox.local,192.168.1.20")
# The certificate has serverAuth extended key usage and a validity below 825 days, which
# macOS requires before it will trust it.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ZA_RUN_DIR="${ZA_RUN_DIR:-$REPO_ROOT/.run}"
DAYS=800

force=0
case "${1:-}" in
  "") ;;
  --force) force=1 ;;
  *)
    echo "Usage: $0 [--force]" >&2
    exit 2
    ;;
esac
if [ "$#" -gt 1 ]; then
  echo "Usage: $0 [--force]" >&2
  exit 2
fi

mkdir -p "$ZA_RUN_DIR"
ZA_RUN_DIR="$(cd "$ZA_RUN_DIR" && pwd)"
TLS_DIR="$ZA_RUN_DIR/tls"
CERT="$TLS_DIR/cert.pem"
KEY="$TLS_DIR/key.pem"

if [ "$force" -eq 0 ]; then
  for f in "$CERT" "$KEY"; do
    if [ -e "$f" ]; then
      echo "tls-cert.sh: $f already exists; pass --force to overwrite." >&2
      exit 1
    fi
  done
fi

# Subject alternative names: the fixed ones, then the extras.
dns=(localhost)
ips=(127.0.0.1 ::1)
if [ -n "${ZA_TLS_EXTRA_SAN:-}" ]; then
  IFS=',' read -r -a extras <<<"$ZA_TLS_EXTRA_SAN"
  for raw in ${extras[@]+"${extras[@]}"}; do
    name="${raw//[[:space:]]/}"
    [ -n "$name" ] || continue
    if [[ ! $name =~ ^[A-Za-z0-9._:*-]+$ ]]; then
      echo "tls-cert.sh: ZA_TLS_EXTRA_SAN entry '$name' is not a DNS name or IP address." >&2
      exit 1
    fi
    if [[ $name =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ || $name == *:* ]]; then
      ips+=("$name")
    else
      dns+=("$name")
    fi
  done
fi

mkdir -p "$TLS_DIR"
work="$(mktemp -d "$TLS_DIR/.gen.XXXXXX")"
trap 'rm -rf "$work"' EXIT

{
  echo '[req]'
  echo 'distinguished_name = dn'
  echo 'prompt = no'
  echo 'x509_extensions = ext'
  echo '[dn]'
  echo 'CN = localhost'
  echo 'O = Stalwart local test server'
  echo '[ext]'
  echo 'basicConstraints = critical,CA:FALSE'
  echo 'keyUsage = critical,digitalSignature,keyEncipherment'
  echo 'extendedKeyUsage = serverAuth'
  echo 'subjectAltName = @alt'
  echo '[alt]'
  n=0
  for name in "${dns[@]}"; do
    n=$((n + 1))
    echo "DNS.$n = $name"
  done
  n=0
  for name in "${ips[@]}"; do
    n=$((n + 1))
    echo "IP.$n = $name"
  done
} >"$work/openssl.cnf"

(
  umask 077
  openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days "$DAYS" \
    -config "$work/openssl.cnf" -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
)
chmod 600 "$work/key.pem"
chmod 644 "$work/cert.pem"
mv -f "$work/key.pem" "$KEY"
mv -f "$work/cert.pem" "$CERT"

echo "certificate: $CERT"
echo "private key: $KEY (mode 0600)"
echo "valid until: $(openssl x509 -in "$CERT" -noout -enddate | sed 's/^notAfter=//')"
echo "names:       ${dns[*]} ${ips[*]}"
echo
echo "To trust it on macOS (Apple Calendar and Safari need this), run:"
printf '  security add-trusted-cert -r trustRoot -k ~/Library/Keychains/login.keychain-db %q\n' "$CERT"
