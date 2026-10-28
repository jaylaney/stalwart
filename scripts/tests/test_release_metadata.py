#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Exercise the real common build script without compiling the server."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
UPSTREAM = "18f4e9505027d790fd7e9fdad7f922be6719101d"
VERSION = "0.1.0-beta.2"


class ReleaseMetadata(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = tempfile.TemporaryDirectory(prefix="stalwart-release-metadata-")
        cls.build_script = Path(cls.scratch.name) / "build-script"
        subprocess.run(
            ["rustc", "--edition=2024", str(ROOT / "crates/common/build.rs"),
             "-o", str(cls.build_script)], check=True,
        )

    @classmethod
    def tearDownClass(cls):
        cls.scratch.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="stalwart-release-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "crates/common").mkdir(parents=True)
        (self.root / "crates/main").mkdir()
        (self.root / "resources/locales").mkdir(parents=True)
        (self.root / "resources/locales/i18n.yml").write_text('hello:\n  en: "hello"\n')
        self.write_metadata()
        (self.root / "tracked-source.txt").write_text("fixture source\n")
        (self.root / "crates/main/Cargo.toml").write_text(
            f'[package]\nname = "stalwart"\nversion = "{VERSION}"\n'
        )
        self.out = self.root / "out"
        self.out.mkdir()

    def write_metadata(self, version=VERSION, upstream=UPSTREAM):
        (self.root / "fork-release.toml").write_text(
            f'version = "{version}"\nupstream_tag = "v0.16.26"\n'
            f'upstream_commit = "{upstream}"\n'
        )

    def run_build(self, **overrides):
        env = {key: value for key, value in os.environ.items()
               if not key.startswith("GIT_") and key != "STALWART_BUILD_SOURCE_COMMIT"}
        env.update(CARGO_MANIFEST_DIR=str(self.root / "crates/common"), OUT_DIR=str(self.out))
        env.update(overrides)
        return subprocess.run([str(self.build_script)], env=env, text=True, capture_output=True)

    def metadata(self, **overrides):
        result = self.run_build(**overrides)
        self.assertEqual(result.returncode, 0, result.stderr)
        return dict(line.removeprefix("cargo:rustc-env=").split("=", 1)
                    for line in result.stdout.splitlines() if line.startswith("cargo:rustc-env="))

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], text=True).strip()

    def init_git(self):
        self.git("init", "-q")
        self.git("config", "user.email", "release-test@example.invalid")
        self.git("config", "user.name", "Release metadata test")
        self.git("add", "fork-release.toml", "crates", "resources", "tracked-source.txt")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def test_shipping_manifest_package_arrays_do_not_end_the_table(self):
        manifest = self.root / "crates/main/Cargo.toml"
        manifest.write_text(
            '[package]\nname = "stalwart"\nauthors = ["Example"]\n'
            'keywords = ["mail", "calendar"]\ncategories = ["email"]\n'
            f'version = "{VERSION}"\n'
            '[[bin]]\nname = "stalwart"\n'
            '[dependencies.example]\nversion = "9.9.9"\n'
        )
        self.assertEqual(self.metadata()["STALWART_FORK_VERSION"], VERSION)
        manifest.write_text((ROOT / "crates/main/Cargo.toml").read_text())
        self.assertEqual(self.metadata()["STALWART_FORK_VERSION"], VERSION)

    def test_archive_has_explicit_unknown_and_prerelease_identity(self):
        metadata = self.metadata()
        self.assertEqual(metadata["STALWART_FORK_VERSION"], VERSION)
        self.assertEqual(metadata["STALWART_UPSTREAM_TAG"], "v0.16.26")
        self.assertEqual(metadata["STALWART_UPSTREAM_COMMIT"], UPSTREAM)
        self.assertEqual(metadata["STALWART_SOURCE_COMMIT"], "unknown")
        self.assertEqual(metadata["STALWART_SOURCE_STATE"], "unknown")
        self.assertEqual(metadata["STALWART_BUILD_IDENTITY"],
                         f"Stalwart Server fork v{VERSION} (upstream v0.16.26 {UPSTREAM}; "
                         "source unknown; unknown)")

    def test_explicit_archive_provenance_and_invalid_override(self):
        source = "abcdef0123456789" * 2 + "abcdef01"
        metadata = self.metadata(STALWART_BUILD_SOURCE_COMMIT=source)
        self.assertEqual(metadata["STALWART_SOURCE_COMMIT"], source)
        self.assertEqual(metadata["STALWART_SOURCE_STATE"], "provided")
        for invalid in ["abc1234", "g" * 40, "", "a" * 40 + "\n"]:
            with self.subTest(invalid=invalid):
                result = self.run_build(STALWART_BUILD_SOURCE_COMMIT=invalid)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("STALWART_BUILD_SOURCE_COMMIT", result.stderr)

    def test_manifest_mismatch_and_invalid_upstream_are_rejected(self):
        self.write_metadata(version="0.2.0")
        self.assertNotEqual(self.run_build().returncode, 0)
        self.write_metadata(upstream="not-a-commit")
        self.assertNotEqual(self.run_build().returncode, 0)

    def test_checkout_refresh_dirty_state_and_override_mismatch(self):
        first = self.init_git()
        self.assertEqual(self.metadata()["STALWART_SOURCE_COMMIT"], first)
        self.assertEqual(self.metadata()["STALWART_SOURCE_STATE"], "clean")
        path = self.root / "resources/locales/i18n.yml"
        path.write_text('hello:\n  en: "changed"\n')
        self.assertEqual(self.metadata()["STALWART_SOURCE_STATE"], "dirty")
        self.git("add", str(path))
        self.git("commit", "-qm", "update")
        second = self.git("rev-parse", "HEAD")
        self.assertNotEqual(first, second)
        self.assertEqual(self.metadata()["STALWART_SOURCE_COMMIT"], second)
        self.assertEqual(self.metadata()["STALWART_SOURCE_STATE"], "clean")
        self.assertNotEqual(self.run_build(STALWART_BUILD_SOURCE_COMMIT=first).returncode, 0)
        output = self.run_build().stdout
        self.assertIn("cargo:rerun-if-env-changed=STALWART_BUILD_SOURCE_COMMIT", output)
        self.assertIn("cargo:rerun-if-changed=", output)
        self.assertIn(str(path), output)

    def test_archive_does_not_inherit_enclosing_repository_identity(self):
        self.init_git()
        child = self.root / "source-archive"
        shutil.copytree(self.root / "crates", child / "crates")
        shutil.copytree(self.root / "resources", child / "resources")
        shutil.copy(self.root / "fork-release.toml", child)
        self.root = child
        self.assertEqual(self.metadata()["STALWART_SOURCE_COMMIT"], "unknown")

    def test_cargo_incremental_refreshes_commit_and_unstaged_dirty_state(self):
        manifest = self.root / "crates/common/Cargo.toml"
        manifest.write_text(
            '[package]\nname = "release-metadata-fixture"\nversion = "0.0.0"\n'
            'edition = "2024"\n' +
            f'build = "{ROOT / "crates/common/build.rs"}"\n'
        )
        source = self.root / "crates/common/src"
        source.mkdir()
        (source / "main.rs").write_text(
            '#[allow(dead_code)]\n' +
            f'#[path = "{ROOT / "crates/common/src/version.rs"}"]\n' +
            'mod version;\nfn main() { println!("{}", version::BUILD_IDENTITY); }\n'
        )
        first = self.init_git()

        def cargo_identity():
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith("GIT_") and key != "STALWART_BUILD_SOURCE_COMMIT"}
            env["CARGO_TARGET_DIR"] = str(self.root / "target")
            return subprocess.check_output(
                ["cargo", "run", "--quiet", "--offline", "--manifest-path", str(manifest)],
                env=env, text=True,
            ).strip()

        self.assertIn(f"source {first}; clean)", cargo_identity())
        path = self.root / "tracked-source.txt"
        original = path.read_text()
        path.write_text('hello:\n  en: "changed"\n')
        self.assertIn(f"source {first}; dirty)", cargo_identity())
        path.write_text(original)
        self.assertIn(f"source {first}; clean)", cargo_identity())
        path.write_text('hello:\n  en: "next commit"\n')
        self.git("add", str(path))
        self.git("commit", "-qm", "new revision")
        second = self.git("rev-parse", "HEAD")
        self.assertIn(f"source {second}; clean)", cargo_identity())
        linked = self.root / "linked"
        self.git("worktree", "add", "-qb", "incremental-release-fixture", str(linked))
        self.root = linked
        manifest = self.root / "crates/common/Cargo.toml"
        self.git("pack-refs", "--all")
        self.assertIn(f"source {second}; clean)", cargo_identity())
        path = self.root / "tracked-source.txt"
        path.write_text('hello:\n  en: "worktree revision"\n')
        self.git("add", str(path))
        self.git("commit", "-qm", "linked revision")
        third = self.git("rev-parse", "HEAD")
        self.assertIn(f"source {third}; clean)", cargo_identity())

    def test_linked_worktree_and_packed_refs_are_watched(self):
        self.init_git()
        linked = self.root / "linked"
        self.git("worktree", "add", "-qb", "release-fixture", str(linked))
        self.root = linked
        self.assertEqual(self.metadata()["STALWART_SOURCE_COMMIT"], self.git("rev-parse", "HEAD"))
        self.git("pack-refs", "--all")
        output = self.run_build().stdout
        self.assertIn(str(Path(self.git("rev-parse", "--git-common-dir")).resolve() /
                          "packed-refs"), output)
        self.assertIn(str(Path(self.git("rev-parse", "--absolute-git-dir")) / "HEAD"), output)


if __name__ == "__main__":
    unittest.main()
