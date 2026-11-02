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


def identity(source=SOURCE, state="clean", upstream=UPSTREAM):
    return (
        f"Stalwart Server fork v{VERSION} (upstream v0.16.26 {upstream}; "
        f"source {source}; {state})"
    )


def git(root, *args):
    return subprocess.check_output(
        ["git", "-C", str(root), "-c", "user.name=Fixture", "-c",
         "user.email=fixture@example.invalid", *args], text=True,
    ).strip()


def commit(root, message):
    (root / "history.txt").write_text(message + "\n")
    git(root, "add", "history.txt")
    git(root, "commit", "-qm", message)
    return git(root, "rev-parse", "HEAD")


class VerifyUpstreamTests(unittest.TestCase):
    """Final review D7: a stale or inconsistent upstream pin is refused."""

    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name) / "fork"
        self.root.mkdir()
        git(self.root, "init", "-q", "-b", "main")
        self.old = commit(self.root, "upstream old")
        git(self.root, "tag", "v1.0.0")
        self.new = commit(self.root, "upstream new")
        git(self.root, "tag", "-a", "-m", "annotated", "v1.1.0")
        self.source = commit(self.root, "fork")
        git(self.root, "checkout", "-q", "-b", "side", self.old)
        self.side = commit(self.root, "unmerged upstream")
        git(self.root, "checkout", "-q", "main")
        pinned = patch.dict(verify_product.PINNED_UPSTREAM_TAGS, clear=True)
        pinned.start()
        self.addCleanup(pinned.stop)

    def verify(self, tag, upstream, source=None):
        verify_product.verify_upstream(
            self.root, {"upstream_tag": tag, "upstream_commit": upstream},
            source or self.source,
        )

    def test_the_latest_merged_tag_passes(self):
        self.verify("v1.1.0", self.new)

    def test_a_tag_that_names_another_commit_is_refused(self):
        with self.assertRaisesRegex(ValueError, "names"):
            self.verify("v1.1.0", self.old)

    def test_a_commit_outside_the_source_history_is_refused(self):
        git(self.root, "tag", "v1.2.0", self.side)
        with self.assertRaisesRegex(ValueError, "not an ancestor"):
            self.verify("v1.2.0", self.side)

    def test_a_stale_pin_is_refused(self):
        # Consistent and in the history, but a later upstream release is merged.
        with self.assertRaisesRegex(ValueError, "stale"):
            self.verify("v1.0.0", self.old)
        git(self.root, "tag", "-d", "v1.1.0")
        self.verify("v1.0.0", self.old)
        verify_product.PINNED_UPSTREAM_TAGS["v1.1.0"] = self.new
        with self.assertRaisesRegex(ValueError, "stale"):
            self.verify("v1.0.0", self.old)

    def test_the_pinned_table_stands_in_for_a_missing_local_tag(self):
        git(self.root, "tag", "-d", "v1.1.0")
        with self.assertRaisesRegex(ValueError, "neither a local tag nor"):
            self.verify("v1.1.0", self.new)
        verify_product.PINNED_UPSTREAM_TAGS["v1.1.0"] = self.new
        self.verify("v1.1.0", self.new)
        verify_product.PINNED_UPSTREAM_TAGS["v1.1.0"] = self.old
        with self.assertRaisesRegex(ValueError, "pinned tag"):
            self.verify("v1.1.0", self.new)

    def test_a_local_tag_and_the_pinned_table_must_agree(self):
        verify_product.PINNED_UPSTREAM_TAGS["v1.1.0"] = self.old
        with self.assertRaisesRegex(ValueError, "pinned tag"):
            self.verify("v1.1.0", self.new)

    def test_a_shallow_checkout_cannot_be_checked(self):
        shallow = self.root.parent / "shallow"
        subprocess.run(["git", "clone", "-q", "--depth", "1", "--no-local", str(self.root),
                        str(shallow)], check=True)
        self.root = shallow
        verify_product.PINNED_UPSTREAM_TAGS["v1.1.0"] = self.new
        with self.assertRaisesRegex(ValueError, "shallow"):
            self.verify("v1.1.0", self.new)


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

    def test_the_real_pins_match_fork_release_metadata(self):
        metadata = verify_product.load_metadata(Path(verify_product.__file__).parents[2])
        self.assertEqual(verify_product.PINNED_UPSTREAM_TAGS.get(metadata["upstream_tag"]),
                         metadata["upstream_commit"])

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
    def write_metadata(root, upstream=UPSTREAM):
        (root / "crates/main").mkdir(parents=True)
        (root / "fork-release.toml").write_text(
            "".join(f'{key} = "{value}"\n'
                    for key, value in {**METADATA, "upstream_commit": upstream}.items())
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
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            upstream = commit(root, "upstream")
            git(root, "tag", "v0.16.26")
            self.write_metadata(root, upstream)
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
                f"print({identity(source, upstream=upstream)!r}, file=stream)\n"
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
            self.assertEqual(provenance["upstream_commit"], upstream)
            self.assertEqual(provenance["target"], "x86_64-unknown-linux-gnu")
            self.assertEqual(provenance["cpu_baseline"], "x86-64-v2")
            self.assertEqual(provenance["features"], ["rocks"])
            self.assertEqual(len(provenance["binary_sha256"]), 64)
            self.assertEqual(provenance["source_state"], "clean")


if __name__ == "__main__":
    unittest.main()
