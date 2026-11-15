#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only

"""Offline tests of the retention build snapshot and dependency cache inputs."""

import base64
import io
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().with_name("retention_artifacts.py")
MTIME = 1_790_000_000_123_456_789


def run_tool(*args, env=None):
    return subprocess.run(
        [sys.executable, "-I", str(SCRIPT), *map(str, args)],
        env=env,
        capture_output=True,
        text=True,
    )


class DependencyCacheTests(unittest.TestCase):
    def test_cache_keys_cover_sources_and_platform_independently_of_build_flags(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            cargo_home = repo / "external-home"
            (repo / ".cargo").mkdir()
            config = repo / ".cargo/config.toml"
            lock = repo / "Cargo.lock"
            lock.write_text("version = 4\n")
            source = '[source.crates-io]\nreplace-with = "mirror"\n'
            config.write_text(source + '[build]\nrustflags = ["-Cdebuginfo=0"]\n')

            def key(lane="instrumented", triple="x86_64-unknown-linux-gnu", **extra_env):
                result = run_tool(
                    "cache-key", "--repo", repo, "--lane", lane, "--target-triple", triple,
                    env={**os.environ, "CARGO_HOME": str(cargo_home), **extra_env},
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                return result.stdout.strip()

            original = key()
            self.assertTrue(original.startswith("retention-download-"))
            self.assertNotEqual(original, key("ordinary"))
            self.assertNotEqual(original, key(triple="aarch64-unknown-linux-gnu"))
            self.assertEqual(original, key(RUSTFLAGS="-Cinstrument-coverage"))
            self.assertNotEqual(key(CARGO_REGISTRIES_CRATES_IO_PROTOCOL="git"),
                                key(CARGO_REGISTRIES_CRATES_IO_PROTOCOL="sparse"))
            config.write_text(source + '[build]\nrustflags = ["-Copt-level=1"]\n')
            self.assertEqual(original, key())
            config.write_text(source.replace("mirror", "other"))
            self.assertNotEqual(original, key())
            config.write_text(source)
            lock.write_text("version = 4\n\n[[package]]\nname = \"changed\"\n")
            self.assertNotEqual(original, key())

    def test_normalization_includes_tracked_sources_and_watched_git_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            git = ["git", "-C", str(repo)]
            subprocess.run([*git, "init", "-q"], check=True)
            source = repo / "source with space.rs"
            source.write_text("example\n")
            subprocess.run([*git, "add", "--", source.name], check=True)
            subprocess.run(
                [*git, "-c", "user.name=Example", "-c", "user.email=example@example.invalid",
                 "commit", "-qm", "Fixture"],
                check=True,
            )
            subprocess.run([*git, "pack-refs", "--all"], check=True)
            timestamp = int(subprocess.check_output([*git, "show", "-s", "--format=%ct"], text=True))
            watched = [source, repo / ".git/HEAD", repo / ".git/index",
                       repo / ".git/refs", repo / ".git/packed-refs"]
            for path in watched:
                os.utime(path, ns=(MTIME, MTIME))
            untracked = repo / "untracked"
            untracked.write_text("unchanged\n")
            os.utime(untracked, ns=(MTIME, MTIME))

            result = run_tool("normalize-checkout", "--repo", repo)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual([path.stat().st_mtime_ns for path in watched],
                             [timestamp * 1_000_000_000] * len(watched))
            self.assertEqual(untracked.stat().st_mtime_ns, MTIME)
            source.write_text("dirty\n")
            self.assertNotEqual(run_tool("normalize-checkout", "--repo", repo).returncode, 0)

    def test_quoted_source_table_is_part_of_key(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / ".cargo").mkdir()
            (repo / "Cargo.lock").write_text("version = 4\n")
            config = repo / ".cargo/config.toml"
            args = ("cache-key", "--repo", repo, "--lane", "instrumented",
                    "--target-triple", "x86_64-unknown-linux-gnu")
            config.write_text('["source".crates-io]\nreplace-with = "mirror"\n')
            before = run_tool(*args)
            config.write_text('["source".crates-io]\nreplace-with = "other"\n')
            after = run_tool(*args)
            self.assertEqual((before.returncode, after.returncode), (0, 0))
            self.assertNotEqual(before.stdout, after.stdout)

    def test_cargo_config_patch_changes_cache_key(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / ".cargo").mkdir()
            (repo / "Cargo.lock").write_text("version = 4\n")
            config = repo / ".cargo/config.toml"
            args = ("cache-key", "--repo", repo, "--lane", "ordinary",
                    "--target-triple", "x86_64-unknown-linux-gnu")
            config.write_text('[patch.crates-io]\nfixture = { git = "https://example.invalid/first" }\n')
            before = run_tool(*args)
            config.write_text('[patch.crates-io]\nfixture = { git = "https://example.invalid/second" }\n')
            after = run_tool(*args)
            self.assertEqual((before.returncode, after.returncode), (0, 0))
            self.assertNotEqual(before.stdout, after.stdout)

    def test_cargo_config_include_produces_no_cache_key(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / ".cargo").mkdir()
            (repo / "Cargo.lock").write_text("version = 4\n")
            config = repo / ".cargo/config.toml"
            (config.parent / "source.toml").write_text('[source.crates-io]\nreplace-with = "mirror"\n')
            for text in (
                'include = ["source.toml"]\n',
                '"include" = [{ path = "source.toml", optional = true }]\n',
                '[[include]]\npath = "source.toml"\n',
            ):
                with self.subTest(config=text):
                    config.write_text(text)
                    result = run_tool(
                        "cache-key", "--repo", repo, "--lane", "ordinary", "--target-triple",
                        "x86_64-unknown-linux-gnu",
                    )
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(result.stdout, "")
                    self.assertIn("Cargo config includes", result.stderr)


@unittest.skipUnless(shutil.which("zstd"), "zstd is required for snapshot transport")
class BuildSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.target = self.root / "producer"
        self.archive = self.root / "snapshot.tar.zst"
        self.manifest = self.root / "build.json"
        self.manifest.write_text(json.dumps({"schema": 1, "commit": "a" * 40}) + "\n")
        self.consumer = self.root / "consumer"
        self.restored_manifest = self.root / "restored/build.json"
        self.output = self.target / "llvm-cov-target/debug/build/fixture-111/output"
        self.make("debug/build/fixture-111/output", "cargo::rerun-if-changed=source.rs\n")
        self.make("debug/build/fixture-111/root-output", "/mnt/target/llvm-cov-target/debug/build/fixture-111/out")
        self.make("debug/build/fixture-111/out/bindings.rs", "build state\n")
        self.make("debug/.fingerprint/fixture-111/invoked.timestamp", "")
        self.binary = self.make("debug/deps/fixture-111", "compiled fixture\n", mode=0o755)
        os.link(self.binary, self.target / "llvm-cov-target/debug/fixture")

    def make(self, path, content, mode=0o644):
        file = self.target / "llvm-cov-target" / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(content)
        file.chmod(mode)
        os.utime(file, ns=(MTIME, MTIME))
        return file

    def pack(self, *args, env=None):
        return run_tool(
            "pack-build", "--manifest", self.manifest, "--target", self.target,
            "--out", self.archive, *args, env=env,
        )

    def unpack(self, archive=None):
        return run_tool(
            "unpack-build", "--archive", archive or self.archive, "--target", self.consumer,
            "--manifest-out", self.restored_manifest,
        )

    def test_round_trip_keeps_cargo_state_modes_hardlinks_and_nanosecond_mtimes(self):
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        self.consumer.mkdir()
        unpacked = self.unpack()
        self.assertEqual(unpacked.returncode, 0, unpacked.stderr)
        self.assertEqual(self.restored_manifest.read_bytes(), self.manifest.read_bytes())
        for reference in (self.binary, self.output):
            copied = self.consumer / reference.relative_to(self.target)
            self.assertEqual(copied.read_bytes(), reference.read_bytes())
            self.assertEqual(copied.stat().st_mtime_ns, MTIME)
            self.assertEqual(stat.S_IMODE(copied.stat().st_mode), stat.S_IMODE(reference.stat().st_mode))
        self.assertTrue(os.path.samefile(
            self.consumer / self.binary.relative_to(self.target),
            self.consumer / "llvm-cov-target/debug/fixture",
        ))

    def test_relative_symlink_in_cargo_target_survives_archive(self):
        relative_link = "llvm-cov-target/debug/examples/fixture"
        link = self.target / relative_link
        link.parent.mkdir()
        link.symlink_to("../deps/fixture-111")
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        unpacked = self.unpack()
        self.assertEqual(unpacked.returncode, 0, unpacked.stderr)
        self.assertEqual(os.readlink(self.consumer / relative_link), "../deps/fixture-111")
        self.assertEqual((self.consumer / relative_link).read_bytes(), self.binary.read_bytes())

    def test_omits_profiles_attempt_evidence_and_unselected_paths(self):
        excluded = [
            "previous.profraw",
            "debug/.fingerprint/fixture-111/_step_attempt.profraw",
            "debug/.fingerprint/fixture-111/output-test-bin-fixture",
            "debug/build/fixture-111/out/report.profdata",
            "debug/build/fixture-111/stderr",
            "debug/incremental/fixture-old",
            "debug/test-stores/example",
            "debug/steps/receipt.json",
        ]
        for path in excluded:
            self.make(path, "old evidence\n")
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        unpacked = self.unpack()
        self.assertEqual(unpacked.returncode, 0, unpacked.stderr)
        for path in excluded:
            self.assertFalse((self.consumer / "llvm-cov-target" / path).exists(), path)
        for name in ("output", "root-output"):
            path = self.output.with_name(name)
            self.assertEqual((self.consumer / path.relative_to(self.target)).read_bytes(),
                             path.read_bytes())

    def test_missing_build_script_state_cannot_be_packed(self):
        self.output.unlink()
        result = self.pack()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("build-script", result.stderr)
        self.assertFalse(self.archive.exists())

    def test_required_text_with_credential_marker_blocks_upload_without_leaking_value(self):
        self.output.write_text("cargo::warning=secret=fixture_never_print_this\n")
        result = self.pack()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("build/fixture-111/output", result.stderr)
        self.assertNotIn("fixture_never_print_this", result.stdout + result.stderr)
        self.assertFalse(self.archive.exists())

    def test_public_header_comments_do_not_match_basic_credentials(self):
        header = self.make(
            "debug/build/fixture-111/out/include/openssl/bio.h",
            "// Basic I/O.\n// Check the X.509 basic constraints extension.\n",
        )
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        unpacked = self.unpack()
        self.assertEqual(unpacked.returncode, 0, unpacked.stderr)
        self.assertEqual((self.consumer / header.relative_to(self.target)).read_bytes(),
                         header.read_bytes())

    def test_basic_user_password_credential_blocks_upload_without_leaking_value(self):
        encoded = base64.b64encode(b"fixture-user:synthetic-password").decode()
        self.assertTrue(encoded.endswith("="))
        for token in (encoded, encoded.rstrip("=")):
            with self.subTest(form="padded" if token.endswith("=") else "unpadded"):
                self.output.write_text(f"cargo::warning=Authorization: Basic {token}\n")
                result = self.pack()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("build/fixture-111/output", result.stderr)
                self.assertNotIn(token, result.stdout + result.stderr)
                self.assertFalse(self.archive.exists())

    def test_bare_explicit_credential_in_binary_blocks_upload_without_leaking_value(self):
        credential = "fixture_never_print_bare_value"
        self.binary.write_bytes(b"prefix\0" + credential.encode() + b"\0suffix")
        result = self.pack(
            "--credential-env", "RETENTION_TEST_CREDENTIAL",
            env={**os.environ, "RETENTION_TEST_CREDENTIAL": credential},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("credential", result.stderr)
        self.assertNotIn(credential, result.stdout + result.stderr)
        self.assertFalse(self.archive.exists())

    def test_existing_target_stays_untouched(self):
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        self.consumer.mkdir()
        stale = self.consumer / "old.txt"
        stale.write_text("keep me")
        result = self.unpack()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(stale.read_text(), "keep me")
        self.assertFalse(self.restored_manifest.exists())

    def test_manifest_output_cannot_replace_the_target_directory(self):
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        self.restored_manifest = self.consumer
        result = self.unpack()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("outside the Cargo target", result.stderr)
        self.assertFalse(self.consumer.exists())

    def test_output_archive_cannot_be_written_into_cargo_target(self):
        self.archive = self.target / "llvm-cov-target/debug/deps/snapshot.tar.zst"
        result = self.pack()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("outside the Cargo target", result.stderr)
        self.assertFalse(self.archive.exists())

    def test_unsafe_member_path_is_refused_before_any_output_is_published(self):
        buff = io.BytesIO()
        with tarfile.open(fileobj=buff, mode="w") as snapshot:
            entry = tarfile.TarInfo("../outside")
            entry.size = 4
            snapshot.addfile(entry, io.BytesIO(b"test"))
        with self.archive.open("wb") as output:
            subprocess.run(["zstd", "-q", "-c"], input=buff.getvalue(), stdout=output, check=True)
        result = self.unpack()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("archive member path", result.stderr)
        self.assertFalse((self.root / "outside").exists())
        self.assertFalse(self.consumer.exists())
        self.assertFalse(self.restored_manifest.exists())


if __name__ == "__main__":
    unittest.main()
