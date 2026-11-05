#!/usr/bin/env bash

#
# SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
#
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
#

# Scan the RocksDB data directory ($ZA_RUN_DIR/rocks.db) for strings that must (or must not)
# be at rest.
# Usage: ./leakcheck.sh <word> [<word> ...]
# Prints, per word, how many data files and decoded blob values contain it. Sealed content: expect 0.
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
# counts for a file when any of the three holds it. The file summary says how many blocks and
# frames were decoded and how many frames could not be: an undecodable frame is content the
# scan did not see.
# A separate read-only ldb dump of column family t scans current blob values at their
# record boundaries. It reads plain values (0x00), size-prefixed LZ4 (0xa1, u32 LE)
# and zstd (0xa2). Encrypted values are counted and make the scan fail.
# Development prerequisites: a RocksDB-compatible ldb and Python 3.14+ (compression.zstd).
# Stop the local test server before running so both passes see the same store.
set -u
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
DB="$ZA_RUN_DIR/rocks.db"
if [ ! -d "$DB" ]; then
  echo "leakcheck.sh: no data store at $DB; a scan of nothing would report 0 for every word." >&2
  exit 1
fi
if ! command -v ldb >/dev/null 2>&1; then
  echo "leakcheck.sh: needs RocksDB's ldb to scan logical blob values." >&2
  exit 1
fi
exec python3 -I - "$DB" "$REPO_ROOT/resources/zstd" "$@" <<'PY'
import glob
import os
import re
import subprocess
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


def lz4_body(src, i, n, *, exact=False):
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
            return bytes(out) if not exact or i == end else None
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


def decode_blob(value):
    if not value:
        return value
    trailer = value[-1]
    if trailer & 0xe0 != 0xa0:
        # The server strips 0x00 and passes legacy unmarked values through.
        return value[:-1] if trailer == 0 else value
    if trailer & 0x10:
        return None
    body = value[:-1]
    if trailer == 0xa0:
        return body
    if trailer == 0xa1 and len(body) >= 4:
        decoded = lz4_body(body, 4, int.from_bytes(body[:4], "little"), exact=True)
        if decoded is not None:
            return decoded
    if trailer == 0xa2:
        decoder = zstd.ZstdDecompressor()
        decoded = decoder.decompress(body)
        if decoder.eof and not decoder.unused_data:
            return decoded
    raise ValueError(f"invalid or unsupported blob codec trailer 0x{trailer:02x}")


def scan_blobs(needles):
    # ldb dump opens read-only, reports iterator errors through its exit status,
    # and emits one hex key/value per line followed by its record count.
    command = ["ldb", f"--db={db}", "--column_family=t", "dump", "--hex"]
    counts = dict.fromkeys(needles, 0)
    scanned = encrypted = invalid = 0
    footer = None
    with subprocess.Popen(command, stdout=subprocess.PIPE) as process:
        for line in process.stdout:
            if footer is not None:
                sys.exit("leakcheck.sh: unexpected output after ldb blob count.")
            if match := re.fullmatch(rb"Keys in range: ([0-9]+)\n", line):
                footer = int(match[1])
                continue
            match = re.fullmatch(rb"0x[0-9A-Fa-f]{64} ==> 0x([0-9A-Fa-f]*)\n", line)
            if not match:
                sys.exit("leakcheck.sh: malformed ldb blob record.")
            try:
                decoded = decode_blob(bytes.fromhex(match[1].decode("ascii")))
            except (ValueError, zstd.ZstdError):
                invalid += 1
                continue
            if decoded is None:
                encrypted += 1
            else:
                scanned += 1
                for word, needle in needles.items():
                    counts[word] += needle in decoded
        if process.wait() != 0:
            sys.exit("leakcheck.sh: ldb blob dump failed.")
    if footer != scanned + encrypted + invalid:
        sys.exit("leakcheck.sh: incomplete ldb blob dump.")
    return counts, scanned, encrypted, invalid


marker = "X-ZA-KEY"
needles = {word: word.encode() for word in words + [marker]}
blob_counts, scanned, encrypted, invalid = scan_blobs(needles)
counts = dict.fromkeys(needles, 0)
for directory, _, files in os.walk(db):
    for name in files:
        with open(os.path.join(directory, name), "rb") as f:
            data = f.read()
        views = [data, *zstd_frames(data)]
        if name.endswith((".sst", ".blob")):
            for block in lz4_blocks(data):
                views.append(block)
                views.extend(zstd_frames(block))
        for word, needle in needles.items():
            if any(needle in view for view in views):
                counts[word] += 1

for word, count in counts.items():
    control = "   (expect > 0 once a key account has written)" if word == marker else ""
    print(f"{word:40} {count} file(s), {blob_counts[word]} blob value(s){control}")
print(f"decoded: {stats['blocks']} LZ4 block(s), {stats['frames'] - stats['undecodable']} zstd "
      f"frame(s); undecodable zstd frames: {stats['undecodable']} (expect 0)")
print(f"blob content scanned: {scanned} decoded values")
if encrypted:
    print(f"blob content not scanned: {encrypted} encrypted values")
if invalid:
    print(f"blob content not scanned: {invalid} undecodable values")
if encrypted or invalid:
    sys.exit(1)
PY
