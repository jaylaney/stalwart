#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Scan the RocksDB data directory ($ZA_RUN_DIR/rocks.db) for strings that must (or must not)
# be at rest.
# Usage: ./leakcheck.sh <word> [<word> ...]
# Prints, per word, how many data files contain it. Sealed content: expect 0.
# Positive control: a non-key account's event title must show up (> 0).
# Values are stored as rkyv archives, LZ4-compressed when large; LZ4 keeps literal
# runs verbatim, so a distinctive word still appears if it is stored in plaintext.
set -u
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
DB="$ZA_RUN_DIR/rocks.db"
if [ ! -d "$DB" ]; then
  echo "leakcheck.sh: no data store at $DB; a scan of nothing would report 0 for every word." >&2
  exit 1
fi
for word in "$@"; do
  n=$(grep -a -r -l -F -- "$word" "$DB" 2>/dev/null | wc -l | tr -d ' ')
  printf '%-40s %s file(s)\n' "$word" "$n"
done
printf '%-40s %s file(s)   (expect > 0 once a key account has written)\n' 'X-ZA-KEY' \
  "$(grep -a -r -l -F -- 'X-ZA-KEY' "$DB" 2>/dev/null | wc -l | tr -d ' ')"
