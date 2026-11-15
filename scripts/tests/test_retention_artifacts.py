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
import time
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().with_name("retention_artifacts.py")
SETUP = SCRIPT.with_name("retention_ci_setup.sh")
MTIME = 1_790_000_000_123_456_789
sys.dont_write_bytecode = True
sys.path.insert(0, str(SCRIPT.parent))
from retention_artifacts import pack_build, unpack_build


def run_tool(*args, env=None):
    return subprocess.run(
        [sys.executable, "-I", str(SCRIPT), *map(str, args)],
        env=env,
        capture_output=True,
        text=True,
    )


class DependencyCacheTests(unittest.TestCase):
    def test_live_usage_is_required_and_reserves_budget_for_both_lanes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            commands = root / "bin"
            commands.mkdir()
            (commands / "gh").write_text(
                '#!/bin/sh\n'
                'test "$*" = "api repos/fixture/example/actions/cache/usage --jq .active_caches_size_in_bytes" || exit 99\n'
                'printf "%s\\n" "$FIXTURE_CACHE_USAGE"\n'
                'exit "$FIXTURE_USAGE_STATUS"\n'
            )
            (commands / "du").write_text(
                '#!/bin/sh\n'
                'printf "%s\\t%s\\n" "$FIXTURE_DU_BYTES" "$2"\n'
                'exit "$FIXTURE_DU_STATUS"\n'
            )
            (commands / "timeout").write_text('#!/bin/sh\nshift 2\nexec "$@"\n')
            (commands / "cargo").write_text('#!/bin/sh\nexit 0\n')
            for command in commands.iterdir():
                command.chmod(0o755)
            cargo_home = root / "cargo"
            (cargo_home / "registry/cache").mkdir(parents=True)
            output = root / "outputs"
            summary = root / "summary"

            def run_setup(operation, usage="9500000000", status=0, size="100000000", du_status=0):
                output.write_text("")
                return subprocess.run(
                    ["bash", str(SETUP), operation],
                    capture_output=True,
                    text=True,
                    env={
                        **os.environ,
                        "PATH": f"{commands}{os.pathsep}{os.environ['PATH']}",
                        "GITHUB_REPOSITORY": "fixture/example",
                        "GITHUB_OUTPUT": str(output),
                        "GITHUB_STEP_SUMMARY": str(summary),
                        "CARGO_HOME": str(cargo_home),
                        "GATES": str(root),
                        "RETENTION_DEADLINE_EPOCH": str(int(time.time()) + 60),
                        "RETENTION_TARGET": "x86_64-unknown-linux-gnu",
                        "FIXTURE_CACHE_USAGE": usage,
                        "FIXTURE_USAGE_STATUS": str(status),
                        "FIXTURE_DU_BYTES": size,
                        "FIXTURE_DU_STATUS": str(du_status),
                    },
                )

            cases = (
                ("query failed", "", 1, "Cache usage request failed", False),
                ("empty", "", 0, "Cache usage response is invalid", False),
                ("malformed", "null", 0, "Cache usage response is invalid", False),
                ("under budget", "9500000000", 0, "9500000000 used of 10000000000", True),
                ("room for one lane", "9800000000", 0, "Insufficient cache room", False),
                ("reported usage", "12433401138", 0, "Insufficient cache room", False),
                ("oversized usage", "18446744073709551616", 0, "Insufficient cache room", False),
            )
            for label, usage, status, log, can_save in cases:
                with self.subTest(label=label):
                    result = run_setup("cache-budget", usage, status)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(log, result.stdout)
                    self.assertEqual(output.read_text(), "room=true\n" if can_save else "")

            for size, du_status in (("100000000", 1), ("bad-size", 0)):
                with self.subTest(size=size, du_status=du_status):
                    result = run_setup("cache-budget", size=size, du_status=du_status)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("Cache download size measurement failed", result.stdout)
                    self.assertEqual(output.read_text(), "")

            result = run_setup("fetch", du_status=1)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Dependency download path sizes are unavailable.", result.stdout)

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

    def test_custom_build_script_executable_alias_and_dep_info_survive(self):
        package = "debug/build/fixture-custom-0123456789abcdef"
        executable = self.make(f"{package}/build_script_main-0123456789abcdef",
                               "compiled custom build script\n", mode=0o755)
        alias = executable.with_name("build-script-main")
        os.link(executable, alias)
        dep_info = self.make(f"{package}/build_script_main-0123456789abcdef.d", "native/main.rs\n")
        unrelated = self.make(f"{package}/build_script_main-not-a-hash", "unrelated diagnostic\n")

        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        unpacked = self.unpack()
        self.assertEqual(unpacked.returncode, 0, unpacked.stderr)
        copied = self.consumer / executable.relative_to(self.target)
        self.assertEqual(copied.read_bytes(), executable.read_bytes())
        self.assertEqual(stat.S_IMODE(copied.stat().st_mode), 0o755)
        self.assertTrue(os.path.samefile(copied, self.consumer / alias.relative_to(self.target)))
        self.assertEqual((self.consumer / dep_info.relative_to(self.target)).read_bytes(),
                         dep_info.read_bytes())
        self.assertFalse((self.consumer / unrelated.relative_to(self.target)).exists())

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

    def test_public_sqlite_bindings_and_type_declarations_are_preserved(self):
        bindings = self.make(
            "debug/build/fixture-111/out/bindgen.rs",
            'pub const FTS5_TOKENIZE_QUERY: i32 = 1;\n'
            'pub const FTS5_TOKENIZER: &str = "unicode61";\n'
            'pub pToken: *const ::std::os::raw::c_char,\n'
            '// Bearer authentication is supported.\n'
            '// Example string: "Bearer sample_token"\n'
            '/* Sample value:\n'
            '  "password=example"\n'
            '*/\n'
            'pub const HELP: &str = r##"\n'
            'This contains a quote ".\n'
            '"##; // "Bearer sample_token"\n',
        )
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        restored = self.unpack()
        self.assertEqual(restored.returncode, 0, restored.stderr)
        self.assertEqual((self.consumer / bindings.relative_to(self.target)).read_bytes(),
                         bindings.read_bytes())

    def test_generated_sources_with_quoted_credentials_block_upload(self):
        credential = "fixture_never_print_generated_source_value"
        declarations = (
            f'const API_TOKEN: &\'static str = "{credential}";',
            f'const char *clientSecret = "{credential}";',
            f'let header = "Authorization: Bearer {credential}";',
            f'let header = "Bearer {credential}";',
            f'let config = "password={credential}";',
            f'let secretKey = "{credential}";',
            f'let passwordValue = "{credential}";',
        )
        for declaration in declarations:
            with self.subTest(assignment=declaration.split('"')[0]):
                self.archive.unlink(missing_ok=True)
                bindings = self.make("debug/build/fixture-111/out/bindgen.rs", declaration)
                result = self.pack()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(bindings.name, result.stderr)
                self.assertNotIn(credential, result.stdout + result.stderr)
                self.assertFalse(self.archive.exists())

    def test_generated_multiline_strings_with_credentials_block_upload(self):
        credential = "fixture_never_print_multiline_source_value"
        declarations = (
            ("raw", f'let config = r###"\nAn interior quote "##.\npassword={credential}\n"###;'),
            ("standard", f'let config = "An escaped quote \\".\nBearer {credential}\n";'),
        )
        for syntax, declaration in declarations:
            with self.subTest(syntax=syntax):
                self.archive.unlink(missing_ok=True)
                bindings = self.make("debug/build/fixture-111/out/bindgen.rs", declaration)
                result = self.pack()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(bindings.name, result.stderr)
                self.assertNotIn(credential, result.stdout + result.stderr)
                self.assertFalse(self.archive.exists())

    def test_snapshot_reports_multiple_flagged_paths_before_starting_compression(self):
        credential = "fixture_report_preflight_never_print_this"
        first = self.make("debug/build/fixture-111/out/first.rs", f'const TOKEN: &str = "{credential}";')
        second = self.make("debug/build/fixture-111/out/second.txt", f"password={credential}")
        with mock.patch("retention_artifacts.subprocess.Popen", side_effect=AssertionError(
            "compression started before credential preflight"
        )):
            with self.assertRaises(ValueError) as raised:
                pack_build(self.manifest, self.target, self.archive, [])
        message = str(raised.exception)
        for path in (first, second):
            self.assertIn(str(path.relative_to(self.target)), message)
        self.assertNotIn(credential, message)
        self.assertFalse(self.archive.exists())

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

    def test_precreated_target_restores_under_read_only_parent(self):
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        parent = self.root / "read-only-parent"
        self.consumer = parent / "consumer"
        self.consumer.mkdir(parents=True)
        self.addCleanup(parent.chmod, stat.S_IMODE(parent.stat().st_mode))
        parent.chmod(0o555)
        if os.access(parent, os.W_OK):
            self.skipTest("process can write to a directory with no write permission bits")

        restored = self.unpack()
        self.assertEqual(restored.returncode, 0, restored.stderr)
        self.assertEqual(sorted(path.name for path in self.consumer.iterdir()), ["llvm-cov-target"])
        self.assertEqual(self.restored_manifest.read_bytes(), self.manifest.read_bytes())

    def test_manifest_publication_failure_rolls_back_build(self):
        packed = self.pack()
        self.assertEqual(packed.returncode, 0, packed.stderr)
        self.consumer.mkdir()
        replace = os.replace

        def fail_manifest(source, destination):
            if destination == self.restored_manifest:
                raise PermissionError("fixture publication failure")
            return replace(source, destination)

        with mock.patch("retention_artifacts.os.replace", side_effect=fail_manifest):
            with self.assertRaisesRegex(PermissionError, "fixture publication failure"):
                unpack_build(self.archive, self.consumer, self.restored_manifest)
        self.assertEqual(list(self.consumer.iterdir()), [])
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
