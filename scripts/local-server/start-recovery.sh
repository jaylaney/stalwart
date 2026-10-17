#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Phase 1: first boot in recovery mode (single plain-HTTP listener on 8080, no privileged ports,
# no default listeners written). Run provision.sh against it, then stop it and use start.sh.
# Runs in the foreground from the state directory ($ZA_RUN_DIR); see env.sh for the settings.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
if [ ! -x "$STALWART_BIN" ]; then
  echo "start-recovery.sh: $STALWART_BIN not found; build it first (see README.md)." >&2
  exit 1
fi
cd "$ZA_RUN_DIR"
STALWART_RECOVERY_MODE=1 STALWART_RECOVERY_MODE_PORT=8080 exec "$STALWART_BIN" --config "$ZA_RUN_DIR/datastore.json"
