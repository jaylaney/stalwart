# SPDX-FileCopyrightText: 2026 Jay Laney
# SPDX-License-Identifier: AGPL-3.0-only

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import verify_product


VERSION = "0.1.0-beta.1"
UPSTREAM = "18f4e9505027d790fd7e9fdad7f922be6719101d"
SOURCE = "a" * 40
METADATA = {"version": VERSION, "upstream_tag": "v0.16.26", "upstream_commit": UPSTREAM}


def identity(source=SOURCE, state="clean"):
    return (
        f"Stalwart Server fork v{VERSION} (upstream v0.16.26 {UPSTREAM}; "
        f"source {source}; {state})"
    )


class VerifyProductTests(unittest.TestCase):
    def validate(self, version=None, help_text=None, source=SOURCE, ref=None):
        verify_product.validate_identity(
            METADATA,
            identity() if version is None else version,
            identity() + "\nUsage: stalwart" if help_text is None else help_text,
            source,
            ref or f"refs/tags/fork-v{VERSION}",
        )

    def test_matching_prerelease_tag_and_full_identity(self):
        self.validate()

    def test_bare_tag_from_manual_dispatch_is_checked(self):
        self.validate(ref=f"fork-v{VERSION}")
        with self.assertRaises(ValueError):
            self.validate(ref="fork-v0.1.0-beta.2")

    def test_branch_build_accepts_matching_identity(self):
        self.validate(ref="refs/heads/main")

    def test_mismatched_or_missing_version_is_refused(self):
        for value in ("Stalwart Server v0.16.26", "", identity().replace(VERSION, "0.2.0")):
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.validate(version=value)

    def test_wrong_commit_and_unknown_or_dirty_source_are_refused(self):
        for value in (identity("b" * 40), identity("unknown"), identity(state="dirty")):
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.validate(version=value)

    def test_mismatched_tag_is_refused(self):
        with self.assertRaises(ValueError):
            self.validate(ref="refs/tags/fork-v0.1.0-beta.2")

    def test_unrelated_tag_is_refused(self):
        with self.assertRaises(ValueError):
            self.validate(ref="refs/tags/v0.16.26")

    def test_help_must_start_with_identical_identity(self):
        with self.assertRaises(ValueError):
            self.validate(help_text="upstream help\n" + identity())

    def test_source_must_be_a_full_hex_commit(self):
        for source in ("abc123", "z" * 40, ""):
            with self.subTest(source=source), self.assertRaises(ValueError):
                self.validate(source=source)

    def test_metadata_and_manifest_versions_must_match(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_metadata(root)
            (root / "crates/main/Cargo.toml").write_text(
                '[package]\nname = "stalwart"\nversion = "0.16.26"\n'
            )
            with self.assertRaises(ValueError):
                verify_product.load_metadata(root)

    def test_required_metadata_cannot_be_missing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_metadata(root)
            (root / "fork-release.toml").write_text(f'version = "{VERSION}"\n')
            with self.assertRaises(ValueError):
                verify_product.load_metadata(root)

    @staticmethod
    def write_metadata(root):
        (root / "crates/main").mkdir(parents=True)
        (root / "fork-release.toml").write_text(
            "".join(f'{key} = "{value}"\n' for key, value in METADATA.items())
        )
        (root / "crates/main/Cargo.toml").write_text(
            f'[package]\nname = "stalwart"\nversion = "{VERSION}"\n'
        )

    def test_glibc_requirement_uses_numeric_version_order(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".cargo").mkdir()
            (root / ".cargo/config.toml").write_text(
                "[target.'cfg(all(target_arch = \"x86_64\", "
                "not(target_env = \"musl\")))']\n"
                'rustflags = ["-C", "target-cpu=x86-64-v2"]\n'
            )
            with patch.object(verify_product, "command_output", return_value=(
                "Name: GLIBC_2.9\nName: GLIBC_2.40\nName: GLIBC_ABI_DT_RELR"
            )):
                self.assertEqual(verify_product.minimum_glibc(root / "binary", root), "2.40")
            with patch.object(verify_product, "command_output", return_value="no symbol versions"):
                with self.assertRaises(ValueError):
                    verify_product.minimum_glibc(root / "binary", root)

    def test_cli_reads_help_on_stderr_without_config_and_writes_provenance(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_metadata(root)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "add", "."], check=True)
            subprocess.run(
                ["git", "-C", str(root), "-c", "user.name=Fixture", "-c",
                 "user.email=fixture@example.invalid", "commit", "-qm", "fixture"],
                check=True,
            )
            source = subprocess.check_output(
                ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
            ).strip()
            binary = root / "stalwart"
            binary.write_text(
                f"#!{sys.executable}\nimport os, sys\n"
                "assert 'CONFIG_PATH' not in os.environ\n"
                "stream = sys.stderr if sys.argv[1] == '--help' else sys.stdout\n"
                f"print({identity(source)!r}, file=stream)\n"
            )
            binary.chmod(0o755)
            output = root / "stalwart.provenance.json"
            command = [
                sys.executable, str(Path(verify_product.__file__)), "--root", str(root),
                "--binary", str(binary), "--source-commit", source,
                "--ref", f"refs/tags/fork-v{VERSION}", "--output", str(output),
            ]
            result = subprocess.run(
                command, env={**os.environ, "CONFIG_PATH": "must-not-be-read"},
                text=True, capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            provenance = json.loads(output.read_text())
            self.assertEqual(provenance["source_commit"], source)
            self.assertEqual(provenance["fork_version"], VERSION)
            self.assertEqual(provenance["upstream_commit"], UPSTREAM)
            self.assertEqual(provenance["target"], "x86_64-unknown-linux-gnu")
            self.assertEqual(provenance["cpu_baseline"], "x86-64-v2")
            self.assertEqual(provenance["features"], ["rocks"])
            self.assertEqual(len(provenance["binary_sha256"]), 64)
            self.assertEqual(provenance["source_state"], "clean")


if __name__ == "__main__":
    unittest.main()
