#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Jay Laney
# SPDX-License-Identifier: AGPL-3.0-only
"""Verify the shipping binary's fork identity and write its release provenance (Python 3.11+)."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tomllib


TARGET = "x86_64-unknown-linux-gnu"
CPU_BASELINE = "x86-64-v2"
COMMIT = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")


def load_metadata(root):
    with (root / "fork-release.toml").open("rb") as source:
        metadata = tomllib.load(source)
    for key in ("version", "upstream_tag", "upstream_commit"):
        if not isinstance(metadata.get(key), str) or not metadata[key]:
            raise ValueError(f"fork-release.toml requires nonempty {key}")
    if not COMMIT.fullmatch(metadata["upstream_commit"]):
        raise ValueError("upstream_commit must be a full lowercase hexadecimal commit")
    with (root / "crates/main/Cargo.toml").open("rb") as source:
        manifest = tomllib.load(source)
    if manifest.get("package", {}).get("version") != metadata["version"]:
        raise ValueError("shipping package version does not match fork-release.toml")
    return metadata


def validate_identity(metadata, version_output, help_output, source_commit, ref):
    if not COMMIT.fullmatch(source_commit):
        raise ValueError("source commit must be a full lowercase hexadecimal commit")
    expected = (
        f"Stalwart Server fork v{metadata['version']} "
        f"(upstream {metadata['upstream_tag']} {metadata['upstream_commit']}; "
        f"source {source_commit}; clean)"
    )
    if version_output.strip() != expected:
        raise ValueError(f"binary --version does not match clean release identity: {expected}")
    if not help_output.splitlines() or help_output.splitlines()[0] != expected:
        raise ValueError("binary --help must start with the same release identity")
    if ref.startswith("fork-v"):
        ref = "refs/tags/" + ref
    if ref.startswith("refs/tags/") and ref != f"refs/tags/fork-v{metadata['version']}":
        raise ValueError("release tag does not match fork-release.toml version")


def command_output(command, **kwargs):
    return subprocess.check_output(command, text=True, timeout=30, **kwargs).strip()


def verify_checkout(root, source_commit):
    prefix = ["git", "-C", str(root)]
    actual_root = Path(command_output(prefix + ["rev-parse", "--show-toplevel"])).resolve()
    if actual_root != root:
        raise ValueError("release root must be its own Git checkout")
    if command_output(prefix + ["rev-parse", "HEAD"]) != source_commit:
        raise ValueError("requested source commit does not match checked-out HEAD")
    if command_output(prefix + ["status", "--porcelain", "--untracked-files=no"]):
        raise ValueError("release checkout has tracked modifications")


def minimum_glibc(binary, root):
    with (root / ".cargo/config.toml").open("rb") as source:
        config = tomllib.load(source)
    flags = config["target"]["cfg(all(target_arch = \"x86_64\", not(target_env = \"musl\")))"][
        "rustflags"
    ]
    if ["-C", f"target-cpu={CPU_BASELINE}"] != flags:
        raise ValueError("CPU baseline changed; update release compatibility metadata")
    versions = re.findall(
        r"\bGLIBC_(\d+(?:\.\d+)+)\b",
        command_output(["readelf", "--version-info", str(binary)]),
    )
    if not versions:
        raise ValueError("shipping GNU/Linux binary has no inspectable GLIBC requirement")
    return max(versions, key=lambda value: tuple(map(int, value.split("."))))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--ref", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--inspect-elf", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    binary = args.binary.resolve()
    try:
        metadata = load_metadata(root)
        verify_checkout(root, args.source_commit)
        binary_environment = os.environ.copy()
        binary_environment.pop("CONFIG_PATH", None)
        version = command_output([str(binary), "--version"], env=binary_environment)
        help_text = command_output([str(binary), "--help"], env=binary_environment)
        validate_identity(metadata, version, help_text, args.source_commit, args.ref)
        glibc = minimum_glibc(binary, root) if args.inspect_elf else None
        libc_name, libc_version = platform.libc_ver()
        provenance = {
            "fork_version": metadata["version"],
            "release_tag": f"fork-v{metadata['version']}",
            "upstream_tag": metadata["upstream_tag"],
            "upstream_commit": metadata["upstream_commit"],
            "source_commit": args.source_commit,
            "source_state": "clean",
            "build_identity": version,
            "target": TARGET,
            "cpu_baseline": CPU_BASELINE,
            "default_features": False,
            "features": ["rocks"],
            "build_host": platform.platform(),
            "build_host_libc": {"name": libc_name, "version": libc_version},
            "minimum_glibc_symbol_version": glibc,
            "binary": binary.name,
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        }
        args.output.write_text(json.dumps(provenance, indent=2) + "\n")
        print(version)
        if glibc:
            print(f"Required GLIBC symbol version: {glibc}; CPU baseline: {CPU_BASELINE}")
    except (ValueError, KeyError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Product release verification failed: {error}\n")


if __name__ == "__main__":
    main()
