#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Shared settings for the local manual-test server. Sourced by the other scripts in this
# folder; it sets no shell options, so the sourcing script's own stay in force.
#
# Per-machine state (data store, TLS files, logs, local.env) lives in ZA_RUN_DIR, which
# defaults to .run/ in the repository (git-ignored). Per-machine secrets go in
# $ZA_RUN_DIR/local.env, which is sourced here and never tracked.
#
# Overridable from the environment: ZA_RUN_DIR, STALWART_BIN, BASE, ZA_ACCOUNT_PAGE_ORIGIN.
# Required (usually from local.env): STALWART_RECOVERY_ADMIN.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ZA_RUN_DIR="${ZA_RUN_DIR:-$REPO_ROOT/.run}"

# The state directory must exist before local.env can be put in it.
mkdir -p "$ZA_RUN_DIR"
ZA_RUN_DIR="$(cd "$ZA_RUN_DIR" && pwd)"

if [ -f "$ZA_RUN_DIR/local.env" ]; then
  # shellcheck source=/dev/null
  source "$ZA_RUN_DIR/local.env"
fi

STALWART_BIN="${STALWART_BIN:-$REPO_ROOT/target/release/stalwart}"
BASE="${BASE:-https://127.0.0.1:8443}"

if [ -z "${STALWART_RECOVERY_ADMIN:-}" ]; then
  echo "env.sh: STALWART_RECOVERY_ADMIN is not set." >&2
  echo "Put 'export STALWART_RECOVERY_ADMIN=admin:<password>' in $ZA_RUN_DIR/local.env and run again." >&2
  # exit ends the sourcing script; an interactive shell is only told.
  if [[ $- == *i* ]]; then return 1; fi
  exit 1
fi
export STALWART_RECOVERY_ADMIN

# CORS origin for the account page; the default applies only when the variable is unset.
export ZA_ACCOUNT_PAGE_ORIGIN="${ZA_ACCOUNT_PAGE_ORIGIN-http://localhost:5173}"

# The self-signed certificate of the TLS listener (made by tls-cert.sh).
export CURL_CA_BUNDLE="$ZA_RUN_DIR/tls/cert.pem"

# The data store file the server is started with. Written once, never overwritten.
if [ ! -e "$ZA_RUN_DIR/datastore.json" ]; then
  python3 -I -c 'import json, sys; print(json.dumps({"@type": "RocksDb", "path": sys.argv[1]}, separators=(",", ":")))' \
    "$ZA_RUN_DIR/rocks.db" >"$ZA_RUN_DIR/datastore.json"
fi
