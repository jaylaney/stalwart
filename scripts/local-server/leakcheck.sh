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
#
# At 1.0 a plain byte search no longer sees stored content. The server compresses its
# archives with zstd, most of them against the dictionaries in resources/zstd/, and RocksDB
# then compresses most column families' blocks with LZ4. An LZ4 block keeps literal runs
# verbatim, but a zstd frame is not readable text, and LZ4 back-references can hide a
# frame's header when two frames share a block. So each file is searched three ways: its raw
# bytes, every LZ4 block found in an .sst or .blob file (RocksDB's form: a varint32 size,
# then the LZ4 body; every offset is tried, and only a body that decodes to exactly that
# size counts), and every zstd frame found in the raw bytes or in a decoded block. A word
# counts for a file when any of the three holds it. The last line says how many blocks and
# frames were decoded and how many frames could not be: an undecodable frame is content the
# scan did not see.
# Needs Python 3.14 or later (the standard library's compression.zstd).
set -u
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
DB="$ZA_RUN_DIR/rocks.db"
if [ ! -d "$DB" ]; then
  echo "leakcheck.sh: no data store at $DB; a scan of nothing would report 0 for every word." >&2
  exit 1
fi
exec python3 -I - "$DB" "$REPO_ROOT/resources/zstd" "$@" <<'PY'
import glob
import os
import sys

try:
    from compression import zstd
except ImportError:
    sys.exit("leakcheck.sh: needs Python 3.14 or later (compression.zstd); a raw byte "
             "search alone cannot see 1.0's compressed archives.")

db, dict_dir, *words = sys.argv[1:]
dicts = {}
for path in glob.glob(os.path.join(dict_dir, "*.dict")):
    with open(path, "rb") as f:
        d = zstd.ZstdDict(f.read())
    dicts[d.dict_id] = d
if not dicts:
    sys.exit(f"leakcheck.sh: no zstd dictionaries in {dict_dir}.")

MAGIC = b"\x28\xb5\x2f\xfd"
MIN_BLOCK, MAX_BLOCK = 16, 64 << 20
stats = {"blocks": 0, "frames": 0, "undecodable": 0}


def lz4_body(src, i, n):
    """The LZ4 block body at src[i:] if it decodes to exactly n bytes, else None."""
    out = bytearray()
    end = len(src)
    while True:
        if i >= end:
            return None
        token = src[i]
        i += 1
        literals = token >> 4
        if literals == 15:
            while True:
                if i >= end:
                    return None
                b = src[i]
                i += 1
                literals += b
                if b != 255:
                    break
        if i + literals > end or len(out) + literals > n:
            return None
        out += src[i : i + literals]
        i += literals
        if len(out) == n:
            return bytes(out)
        if i + 2 > end:
            return None
        offset = src[i] | (src[i + 1] << 8)
        i += 2
        if offset == 0 or offset > len(out):
            return None
        length = token & 15
        if length == 15:
            while True:
                if i >= end:
                    return None
                b = src[i]
                i += 1
                length += b
                if b != 255:
                    break
        length += 4
        if len(out) + length > n:
            return None
        start = len(out) - offset
        if offset >= length:
            out += out[start : start + length]
        else:
            for k in range(length):
                out.append(out[start + k])


def lz4_blocks(data):
    for i in range(len(data)):
        n, shift, j = 0, 0, i
        while j < len(data) and j - i < 5:
            b = data[j]
            j += 1
            n |= (b & 0x7F) << shift
            shift += 7
            if not b & 0x80:
                break
        else:
            continue
        if MIN_BLOCK <= n <= MAX_BLOCK:
            body = lz4_body(data, j, n)
            if body is not None:
                stats["blocks"] += 1
                yield body


def zstd_frames(data):
    i = data.find(MAGIC)
    while i >= 0:
        stats["frames"] += 1
        try:
            info = zstd.get_frame_info(data[i : i + 32])
            options = {"zstd_dict": dicts[info.dictionary_id]} if info.dictionary_id else {}
            decompressor = zstd.ZstdDecompressor(**options)
            out = decompressor.decompress(data[i:])
            if not decompressor.eof:
                raise ValueError("truncated frame")
            yield out
        except Exception:
            stats["undecodable"] += 1
        i = data.find(MAGIC, i + 1)


marker = "X-ZA-KEY"
counts = dict.fromkeys(words + [marker], 0)
for directory, _, files in os.walk(db):
    for name in files:
        with open(os.path.join(directory, name), "rb") as f:
            data = f.read()
        views = [data, *zstd_frames(data)]
        if name.endswith((".sst", ".blob")):
            for block in lz4_blocks(data):
                views.append(block)
                views.extend(zstd_frames(block))
        for word in counts:
            needle = word.encode()
            if any(needle in view for view in views):
                counts[word] += 1

for word in words:
    print(f"{word:40} {counts[word]} file(s)")
print(f"{marker:40} {counts[marker]} file(s)   (expect > 0 once a key account has written)")
print(f"decoded: {stats['blocks']} LZ4 block(s), {stats['frames'] - stats['undecodable']} zstd "
      f"frame(s); undecodable zstd frames: {stats['undecodable']} (expect 0)")
PY
