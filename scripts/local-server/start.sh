#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Phase 2: normal mode. Listeners come from the registry (created by provision.sh):
# HTTP on 8080 (plain), HTTPS on 8443 (TLS), IMAP on 1143 (plain).
# Runs in the foreground from the state directory ($ZA_RUN_DIR); see env.sh for the settings.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
if [ ! -x "$STALWART_BIN" ]; then
  echo "start.sh: $STALWART_BIN not found; build it first (see README.md)." >&2
  exit 1
fi
cd "$ZA_RUN_DIR"
exec "$STALWART_BIN" --config "$ZA_RUN_DIR/datastore.json"
