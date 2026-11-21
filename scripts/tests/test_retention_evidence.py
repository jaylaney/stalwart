#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Offline tests for issue #86 producer and collection receipts."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import capture_listings
import ci_steps
import coverage_net
import retention_evidence as evidence
import retention_workflow

LIBTEST_EMPTY = (
    "     Running unittests src/lib.rs (target/debug/deps/sample-1234)\n"
    "0 tests, 0 benchmarks\n"
)
COMMIT = "1" * 40
RUN = dict(commit=COMMIT, run_id="21", producer_attempt=1)
FEATURES = "test/feature test/other"
ARTIFACT = "40"
OBJECTS = [{"path": "debug/deps/example", "sha256": "b" * 64}]
PROFILE = b"\xfflprofi\x81" + (14).to_bytes(8, "little") + b"\x00" * 16 + (64).to_bytes(8, "little") + b"\x00" * 88
EXPORT = {"type": "llvm.coverage.json.export", "version": "2.0.1",
          "data": [{"files": [], "functions": [], "totals": {}}]}
DIAGNOSTICS = (
    ("password=synthetic-password", "password=[REDACTED]"),
    ("token:synthetic-token", "token:[REDACTED]"),
    ("secret_key=synthetic-secret", "secret_key=[REDACTED]"),
    ("Basic synthetic-basic", "Basic [REDACTED]"),
    ("password = fixture-one", "password = [REDACTED]"),
    ("'Secret_Key'\t:\t'fixture two, with punctuation'", "'Secret_Key'\t:\t'[REDACTED]'"),
    (r'{"TOKEN" : "fixture \"three\" and \\four", "port": 80}',
     '{"TOKEN" : "[REDACTED]", "port": 80}'),
    (r"{ secret = 'fixture\', token = 'fixture five' }",
     "{ secret = '[REDACTED]', token = '[REDACTED]' }"),
    ('password = """fixture six "with quotes""""', 'password = "[REDACTED]"'),
    ("token = '''fixture seven 'with quotes''''", "token = '[REDACTED]'"),
    ('password = """fixture start\n' + r'fixture \""" continues' + '\nfixture end""" status=ok',
     'password = "[REDACTED]"\n\n status=ok'),
    ("token = '''fixture\ncontinued''' secret = '''another\nvalue''' done",
     "token = '[REDACTED]'\n secret = '[REDACTED]'\n done"),
)


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(value)


def write_json(path, value):
    write(path, json.dumps(value).encode())


def read_json(path):
    return json.loads(path.read_bytes())


def attempt(status=0, number=1, names=None, profile="collected"):
    return dict(attempt=number, status=status, names=names or [], profile=profile)


class ReceiptTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.inputs = self.root / "inputs"
        self.output = self.root / "gate"
        self.build = self.root / "build.json"
        self.object_set = hashlib.sha256(
            json.dumps(OBJECTS, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        write_json(self.build, dict(schema=1, commit=COMMIT, run_id=RUN["run_id"], build_attempt=1,
                                    identity={"rustc": "rustc test", "cargo": "cargo test"}, objects=OBJECTS,
                                    object_set_sha256=self.object_set))
        self.run = dict(RUN)
        runner = patch.object(evidence, "run_identity", side_effect=lambda: dict(self.run))
        features = patch.object(evidence, "unit_features", return_value=FEATURES)
        for patcher in (runner, features):
            patcher.start()
            self.addCleanup(patcher.stop)

    def producer(self, shard, status="success", change=None):
        source = self.root / "source" / shard
        steps = dict()
        for name in ci_steps.COVERAGE_SHARDS[shard]:
            steps[name] = dict(status="success", attempts=[attempt()], profile=True, export="success")
        if change:
            change(steps)
        for name, step in steps.items():
            if step["profile"]:
                write(source / "profdata" / f"{name}.profdata", PROFILE)
            if step["export"] == "success":
                write_json(source / "steps" / f"{name}.json", EXPORT)
            if step["attempts"]:
                write(source / "logs" / f"{name}.attempt1.log", b"Bearer bad-bearer\npassword=bad-secret\n")
        write_json(source / "shard.json", dict(schema=1, shard=shard, compatible=True,
                                               **self.run, build_attempt=1,
                                               object_set_sha256=self.object_set, steps=steps))
        return evidence.stage("shard", shard, status, source, self.inputs / shard, self.build, ARTIFACT)

    def listings(self, status="success", failure=None, provenance=None, bodies=None):
        source = self.root / "source" / "listings"
        captures = {}
        for rel, config in capture_listings.capture_contract(FEATURES).items():
            sections = config["sections"]
            statuses = [0] * len(sections) if rel != failure else ["not-run"] * len(sections)
            captures[rel] = statuses
            if rel == failure:
                continue
            text = f'# config: {config["label"]}\n# env: {config["environment"]}\n# started: now\n'
            for (title, cmd), result in zip(sections, statuses):
                body = (bodies or {}).get(rel, LIBTEST_EMPTY)
                text += (f"{title}\n## command: {' '.join(cmd)}\n"
                         f"{body}Bearer bad-listing\n## exit: {result}\n")
            write(source / rel, text.encode())
        identity = {"rustc": "rustc test", "cargo": "cargo test"}
        write_json(source / "listing.json", dict(schema=1, identity=identity, unit_features=FEATURES,
                                                  captures=captures, **(provenance or self.run)))
        return evidence.stage("listings", "listings", status, source, self.inputs / "listings")

    def all(self, replace=None):
        replace = replace or {}
        for shard in ci_steps.COVERAGE_SHARDS:
            self.producer(shard, *replace.get(shard, ()))
        self.listings(*replace.get("listings", ()))

    def collect(self, artifact=ARTIFACT):
        with contextlib.redirect_stderr(io.StringIO()):
            return evidence.collect(self.inputs, self.output, self.build, artifact)

    def assertIntegrityError(self, artifact=ARTIFACT):
        self.assertEqual(self.collect(artifact), 2)
        self.assertFalse((self.output / "net").exists())
        self.assertFalse((self.output / "listings").exists())

    def test_complete_matrix_and_older_producer_attempt(self):
        self.all()
        self.run["producer_attempt"] = 2
        self.assertEqual(self.collect(), 0)
        self.assertEqual(len(list((self.output / "net" / "profdata").iterdir())), 19)
        self.assertEqual(len(list((self.output / "listings" / "ign").iterdir())),
                         len(capture_listings.configurations(FEATURES)))
        log = self.inputs / "za-rocks" / "logs" / "za-tests.attempt1.log"
        self.assertNotIn(b"bad-bearer", log.read_bytes())
        self.assertNotIn(b"bad-secret", log.read_bytes())
        listing = next((self.output / "listings" / "list").iterdir())
        self.assertIn("## exit: 0", listing.read_text())
        self.assertNotIn("bad-listing", listing.read_text())

    def test_staged_listings_preserve_test_ids_and_redact_credentials(self):
        entries = (
            "tokenizers::word::tests::indo_european_tokenizer: test",
            "auth::tests::rejects_wrong_password: test",
            "tokens::r#type::password_lookup: benchmark",
        )
        body = "\n".join((
            "     Running unittests src/lib.rs (target/debug/deps/sample-1234)",
            *entries,
            *(original for original, _ in DIAGNOSTICS),
            "",
            "2 tests, 1 benchmark",
            "",
        ))
        paths = ("list/exists__nlp__default.txt", "ign/exists__nlp__default--ignored-only.txt")
        for shard in ci_steps.COVERAGE_SHARDS:
            self.producer(shard)
        self.listings(bodies=dict.fromkeys(paths, body))
        for path in paths:
            staged = (self.inputs / "listings" / path).read_text()
            self.assertTrue(set(entries).issubset(staged.splitlines()))
            for original, expected in DIAGNOSTICS:
                self.assertIn("\n" + expected + "\n", staged)
                self.assertNotIn(original, staged)
            self.assertNotIn("bad-listing", staged)
            self.assertIn("Bearer [REDACTED]", staged.splitlines())
        self.assertEqual(self.collect(), 0)

    def test_exhausted_retry_preserves_complete_coverage_but_fails(self):
        def fail(steps):
            steps["za-tests"].update(status="failure", attempts=[attempt(101), attempt(101, 2)])
        self.all({"za-rocks": ("failure", fail)})
        self.assertEqual(self.collect(), 1)
        self.assertEqual(read_json(self.output / "collection.json")["missing_steps"], [])
        self.assertTrue((self.output / "net" / "steps" / "za-tests.json").is_file())

    def test_incomplete_outcome_and_listing_are_explicit_for_diagnostics(self):
        def fail(steps):
            steps["za-tests"].update(status="failure", attempts=[attempt("deadline", profile="no-data")],
                                     export="not-attempted", profile=False)
        capture = "ign/exists__nlp__default--ignored-only.txt"
        self.all({"za-rocks": ("failure", fail), "listings": ("failure", capture)})
        self.assertEqual(self.collect(), 1)
        result = read_json(self.output / "collection.json")
        self.assertEqual(result["missing_steps"], ["za-tests"])
        self.assertEqual(result["missing_captures"], [capture])
        self.assertFalse((self.output / "net" / "profdata" / "za-tests.profdata").exists())

    def test_failed_export_keeps_its_indexed_profile_for_diagnosis(self):
        def fail(steps):
            steps["unit-tests"].update(status="failure", export="failure")

        self.all({"short": ("failure", fail)})
        self.assertEqual(self.collect(), 1)
        result = read_json(self.output / "collection.json")
        self.assertEqual(result["missing_exports"], ["unit-tests"])
        self.assertEqual(result["missing_profiles"], [])
        self.assertEqual((self.output / "net" / "profdata" / "unit-tests.profdata").read_bytes(),
                         PROFILE)
        self.assertFalse((self.output / "net" / "steps" / "unit-tests.json").exists())

    def test_unit_listing_failure_with_no_attempts_keeps_only_the_owned_log(self):
        relative = "logs/unit-tests.listing.log"

        def fail(steps):
            steps["unit-tests"].update(status="failure", attempts=[], profile=False,
                                       export="not-attempted")

        for shard in ("short", "za-rocks"):
            write(self.root / "source" / shard / relative, b"Bearer bad-listing\ntoken: test\n")
            write(self.root / "source" / shard / "logs/unit-tests.attempt1.listing.log",
                  b"Unexpected log\n")
        self.all({"short": ("failure", fail)})
        owned = self.inputs / "short"
        self.assertTrue((owned / relative).is_file())
        self.assertEqual((owned / relative).read_text(),
                         "Bearer [REDACTED]\ntoken: [REDACTED]\n")
        self.assertIn(relative, read_json(owned / "receipt.json")["files"])
        for directory in (owned, self.inputs / "za-rocks"):
            self.assertFalse((directory / "logs/unit-tests.attempt1.listing.log").exists())
        self.assertFalse((self.inputs / "za-rocks" / relative).exists())
        self.assertFalse((owned / "logs/unit-tests.attempt1.log").exists())
        self.assertEqual(self.collect(), 1)

    def test_unit_listing_diagnostics_preserve_ids_only_from_libtest_stdout(self):
        source = self.root / "source" / "short"
        logs = source / "logs"
        logs.mkdir(parents=True)
        entries = ("tokens::r#type::password_lookup: benchmark", "password: test")
        stdout = "\n".join((*entries, "secret=synthetic-stdout: test",
                            'password = """fixture', "private::looks_like_a_test: test", 'end"""',
                            "1 test, 1 benchmark", ""))
        stderr = "token: test\nBearer synthetic-stderr\n"
        fake_cargo = (
            "import sys\n"
            f"sys.stdout.write({stdout!r})\n"
            f"sys.stderr.write({stderr!r})\n"
            "sys.exit(101)\n"
        )
        net = object.__new__(coverage_net.Net)
        net.out, net.work_deadline = source, None
        with patch.object(coverage_net, "LLVM_TEST", [sys.executable, "-c", fake_cargo]):
            with self.assertRaisesRegex(RuntimeError, r"listing failed \(exit 101,"):
                net.tests_crate_skips()

        def fail(steps):
            steps["unit-tests"].update(status="failure", attempts=[], profile=False,
                                       export="not-attempted")

        for file in logs.glob("unit-tests.listing*.log"):
            write(self.root / "source" / "za-rocks" / "logs" / file.name, file.read_bytes())
        self.all({"short": ("failure", fail)})
        fallback = self.root / "fallback"
        evidence.stage_shard_diagnostics("short", source, fallback)
        for directory in (self.inputs / "short", fallback):
            with self.subTest(publisher=directory.name):
                files = list((directory / "logs").glob("unit-tests.listing*.log"))
                lines = "\n".join(file.read_text() for file in files).splitlines()
                self.assertTrue(set(entries).issubset(lines), lines)
                self.assertIn("token: [REDACTED]", lines)
                self.assertIn("secret=[REDACTED] test", lines)
                self.assertIn("Bearer [REDACTED]", lines)
                self.assertNotIn("private::looks_like_a_test: test", lines)
                self.assertNotIn("token: test", lines)
                self.assertFalse(any("synthetic-" in line for line in lines))
        unowned = self.root / "unowned"
        evidence.stage_shard_diagnostics("za-rocks", self.root / "source" / "za-rocks", unowned)
        for directory in (self.inputs / "za-rocks", unowned):
            self.assertEqual(list((directory / "logs").glob("unit-tests.listing*.log")), [])
        self.assertEqual(self.collect(), 1)

    def test_smtp_failing_names_retry_passes_with_combined_evidence(self):
        def retry(steps):
            steps["smtp"]["attempts"] = [attempt(101, names=["smtp::flaky"]), attempt(0, 2)]
        self.all({"short": ("success", retry)})
        self.assertEqual(self.collect(), 0)
        short = read_json(self.inputs / "short" / "receipt.json")
        self.assertEqual(len(short["ledger"]["steps"]["smtp"]["attempts"]), 2)
        self.assertTrue((self.output / "net" / "profdata" / "smtp.profdata").is_file())

    def test_missing_latest_upload_does_not_fall_back_to_previous_receipt(self):
        self.all()
        (self.inputs / "za-rocks").rename(self.inputs / "old-za-rocks")
        self.assertIntegrityError()

    def test_duplicate_receipt_is_refused(self):
        self.all()
        shutil.copytree(self.inputs / "short", self.inputs / "unexpected-producer")
        self.assertIntegrityError()

    def test_wrong_snapshot_id_is_refused(self):
        self.all()
        self.assertIntegrityError("99")

    def test_wrong_commit_run_objects_or_attempt_are_refused(self):
        self.all()
        path = self.inputs / "za-rocks" / "receipt.json"
        original = path.read_bytes()
        for key, val in (("commit", "0" * 40), ("run_id", "99"), ("object_set_sha256", "a" * 64),
                         ("producer_attempt", 2), ("build_attempt", 2)):
            with self.subTest(field=key):
                receipt = json.loads(original)
                receipt[key] = val
                write_json(path, receipt)
                self.assertIntegrityError()
                shutil.rmtree(self.output)

    def test_skipped_producer_is_an_integrity_failure(self):
        self.all({"za-rocks": ("skipped",)})
        self.assertIntegrityError()

    def test_damaged_payload_fails_even_with_a_matching_digest(self):
        self.all()
        path = self.inputs / "za-rocks" / "profdata" / "za-tests.profdata"
        write(path, b"no profile")
        receipt_path = self.inputs / "za-rocks" / "receipt.json"
        receipt = read_json(receipt_path)
        receipt["files"]["profdata/za-tests.profdata"] = hashlib.sha256(path.read_bytes()).hexdigest()
        write_json(receipt_path, receipt)
        self.assertIntegrityError()

    def test_unexpected_or_unexplained_missing_file_is_refused(self):
        self.all()
        path = self.inputs / "listings" / "ign" / "exists__nlp__default--ignored-only.txt"
        original = path.read_bytes()
        path.unlink()
        self.assertIntegrityError()
        shutil.rmtree(self.output)
        write(path, original)
        write(self.inputs / "short" / "steps" / "unassigned.json", b"{}")
        self.assertIntegrityError()

    def test_successful_producer_cannot_hide_retries_or_coverage_gaps(self):
        def omit_retry(steps):
            steps["za-tests"].update(status="failure", attempts=[attempt(101)])
        self.all({"za-rocks": ("success", omit_retry)})
        self.assertIntegrityError()

    def test_invalid_ledger_is_never_published_with_safe_diagnostics(self):
        marker = "unexpected-metadata-that-must-not-be-published"

        def invalidate(steps):
            steps["za-tests"].update(status="invalid", unexpected=marker)

        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            self.producer("za-rocks", status="failure", change=invalidate)
        directory = self.inputs / "za-rocks"
        receipt = read_json(directory / "receipt.json")
        self.assertTrue(receipt["integrity_error"])
        self.assertNotIn("ledger", receipt)
        self.assertNotIn(marker, stderr.getvalue())
        self.assertNotIn(marker, (directory / "receipt.json").read_text())
        diagnostic = (directory / "logs" / "za-tests.attempt1.log").read_text()
        self.assertIn("Bearer [REDACTED]", diagnostic)
        self.assertNotIn("bad-bearer", diagnostic)
        self.assertFalse((directory / "profdata").exists())

    def test_stage_refuses_to_relabel_stale_listing_capture(self):
        for key, value in (("commit", "7" * 40), ("run_id", "20"), ("producer_attempt", 1)):
            with self.subTest(field=key):
                self.run["producer_attempt"] = 2
                self.listings(provenance=dict(self.run, **{key: value}))
                directory = self.inputs / "listings"
                receipt = read_json(directory / "receipt.json")
                shutil.rmtree(directory)
                self.assertIs(receipt.get("integrity_error"), True)
                self.assertEqual(receipt["files"], {})

    def test_collector_requires_listing_ledger_to_match_its_receipt(self):
        self.all()
        self.run["producer_attempt"] = 2
        path = self.inputs / "listings" / "receipt.json"
        receipt = read_json(path)
        receipt["producer_attempt"] = 2
        write_json(path, receipt)
        self.assertIntegrityError()
        self.assertIn("ledger differs in producer_attempt",
                      read_json(self.output / "collection.json")["errors"][0])

    def test_listing_commands_and_features_are_checked_after_transfer(self):
        self.all()
        relative = "list/test.yml__test__smtp-tests.txt"
        path = self.inputs / "listings" / relative
        path.write_text(path.read_text().replace(" --nocapture --list", " --nocapture --ignored --list"))
        receipt_path = self.inputs / "listings" / "receipt.json"
        receipt = read_json(receipt_path)
        receipt["files"][relative] = hashlib.sha256(path.read_bytes()).hexdigest()
        write_json(receipt_path, receipt)
        self.assertIntegrityError()
        summary = read_json(self.output / "collection.json")
        self.assertIn("contradicts its capture contract", summary["errors"][0])

    def test_bodyless_ignored_listing_fails_even_with_a_matching_digest(self):
        self.all()
        relative = "ign/exists__nlp__default--ignored-only.txt"
        directory = self.inputs / "listings"
        path = directory / relative
        path.write_text("\n".join(
            line for line in path.read_text().splitlines() if line.startswith("#")
        ) + "\n")
        receipt_path = directory / "receipt.json"
        receipt = read_json(receipt_path)
        receipt["files"][relative] = hashlib.sha256(path.read_bytes()).hexdigest()
        write_json(receipt_path, receipt)
        self.assertIntegrityError()
        self.assertIn("incomplete libtest listing",
                      read_json(self.output / "collection.json")["errors"][0])

    def test_successful_listing_cannot_end_with_an_unfinished_target(self):
        self.all()
        relative = "list/exists__nlp__default.txt"
        directory = self.inputs / "listings"
        path = directory / relative
        path.write_text(path.read_text().replace(
            "## exit: 0",
            "     Running tests/integration.rs (target/debug/deps/integration-5678)\n## exit: 0",
        ))
        receipt_path = directory / "receipt.json"
        receipt = read_json(receipt_path)
        receipt["files"][relative] = hashlib.sha256(path.read_bytes()).hexdigest()
        write_json(receipt_path, receipt)
        self.assertIntegrityError()

    def test_stale_output_is_rejected_without_overwriting(self):
        self.all()
        stale = self.output / "unrelated"
        write(stale, b"old")
        self.assertIntegrityError()
        self.assertEqual(stale.read_bytes(), b"old")


class ShardDiagnosticTests(unittest.TestCase):
    def test_stage_logs_redacts_assignments_and_preserves_other_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "build" / "steps.tsv"
            out = root / "staged"
            control = b"\xff\x00\tcompiler diagnostic\r\n"
            write(source, control + b"".join((line + "\r\n").encode() for line, _ in DIAGNOSTICS)
                  + b'Bearer fixture-bearer\r\npassword = """fixture\r\nfixture at EOF')
            setup = Path(__file__).resolve().with_name("retention_ci_setup.sh")
            subprocess.run(["bash", str(setup), "stage-logs", str(out), str(source),
                            str(root / "missing.log")], cwd=root, check=True)
            self.assertEqual([p.name for p in out.iterdir()], ["steps.tsv"])
            self.assertEqual((out / source.name).read_bytes(), control
                             + b"".join((line + "\r\n").encode() for _, line in DIAGNOSTICS)
                             + b'Bearer [REDACTED]\r\npassword = "[REDACTED]"\r\n')

    def test_stage_safe_diagnostics_without_usable_receipt_inputs(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / "net"
            out = Path(temp) / "diagnostics"
            write(source / "steps.tsv", b"password=bad-password\n")
            write(source / "logs/za-tests.attempt1.log", b"Bearer bad-za\n")
            write(source / "logs/unit-tests.listing.log", b"token=bad-token\n")
            write(source / "logs/unit-tests.extra.log", b"unowned-filename\n")
            write(source / "profdata/unit-tests.profdata", PROFILE)
            write(source / "shard.json", b"unsettled")
            (source / "logs/za-tests.export.log").symlink_to(source / "shard.json")
            driver = Path(__file__).with_name("retention_artifacts.py")
            command = [sys.executable, "-I", str(driver), "stage-shard-diagnostics", "--source",
                       str(source), "--out", str(out)]
            for shard, expected in {
                "short": {"logs/unit-tests.listing.log": "token=[REDACTED]\n"},
                "za-rocks": {"logs/za-tests.attempt1.log": "Bearer [REDACTED]\n"},
            }.items():
                with self.subTest(shard=shard):
                    result = subprocess.run(command + ["--producer", shard], capture_output=True,
                                            text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    staged = {str(file.relative_to(out)): file.read_text()
                              for file in out.rglob("*") if file.is_file()}
                    self.assertEqual(staged, {"steps.tsv": "password=[REDACTED]\n", **expected})
                    shutil.rmtree(out)


class RetentionTriggerTests(unittest.TestCase):
    def setUp(self):
        self.workflow = ci_steps.load_yaml(ci_steps.WORKFLOW.read_text())
        callee = ci_steps.WORKFLOW.with_name(retention_workflow.SHARD_WORKFLOW_NAME)
        self.callee = ci_steps.load_yaml(callee.read_text())

    def check(self, triggers):
        return retention_workflow.check({**self.workflow, "on": triggers}, self.callee,
                                        ci_steps.BASE_ENV, ci_steps.COVERAGE_SHARDS)

    def test_an_extra_event_cannot_run_the_ordinary_jobs(self):
        problems = self.check({**self.workflow["on"], "repository_dispatch": None})
        self.assertIn("test.yml: on.repository_dispatch: unexpected", problems)

    def test_ordinary_trigger_filter_edits_preserve_retention_contract(self):
        for event in ("push", "pull_request"):
            with self.subTest(event=event):
                triggers = dict(self.workflow["on"])
                triggers[event] = {"branches": ["main", "feature"], "paths": ["crates/**"]}
                self.assertEqual(self.check(triggers), [])
                del triggers[event]
                self.assertIn(f"test.yml: on.{event}: missing", self.check(triggers))


class ListingBodyTests(unittest.TestCase):
    def validate(self, body, status=0):
        relative = "list/exists__nlp__default.txt"
        spec = capture_listings.capture_contract(FEATURES)[relative]
        title, cmd = spec["sections"][0]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "capture.txt"
            path.write_text(f'# config: {spec["label"]}\n# env: {spec["environment"]}\n'
                            f"{title}\n## command: {' '.join(cmd)}\n"
                            f"{body}## exit: {status}\n")
            evidence.validate_listing(path, relative, spec, [status])

    def test_typical_cargo_targets_and_singular_footer(self):
        self.validate(
            LIBTEST_EMPTY
            + "     Running tests/integration.rs (target/debug/deps/integration-5678)\n"
            + "suite::single: test\nsuite::benchmark: benchmark\n\n1 test, 1 benchmark\n"
        )

    def test_listing_redaction_protects_only_identifiers_inside_cargo_targets(self):
        with tempfile.TemporaryDirectory() as temp:
            source, staged = Path(temp) / "capture.txt", Path(temp) / "staged.txt"
            source.write_text(
                "password: test\n"
                "## command: cargo test -- --list\n"
                "password: test\n"
                "     Running unittests src/lib.rs (target/debug/deps/sample-1234)\n"
                "token=synthetic-token: test\n"
                "auth::password: bench\n"
                "0 tests, 1 benchmark\n"
                "password: test\n"
                "## exit: 0\n"
                "password: test\n"
            )
            evidence.redacted(source, staged, listing=True)
            redacted = staged.read_text().splitlines()
            self.assertIn("auth::password: bench", redacted)
            self.assertNotIn("password: test", redacted)
            self.assertNotIn("synthetic-token", staged.read_text())
            self.assertEqual(redacted.count("password: [REDACTED]"), 4)

    def test_standalone_zero_test_footer_is_complete(self):
        self.validate("0 tests, 0 benchmarks\n")

    def test_intermediate_target_must_also_settle(self):
        with self.assertRaisesRegex(ValueError, "incomplete libtest listing"):
            self.validate("     Running unittests src/lib.rs (target/debug/deps/first-1234)\n"
                          + LIBTEST_EMPTY)

    def test_footer_counts_must_match_discovered_entries(self):
        with self.assertRaisesRegex(ValueError, "incomplete libtest listing"):
            self.validate("1 test, 0 benchmarks\n")

    def test_timeout_can_preserve_a_partial_target_listing(self):
        self.validate("     Running unittests src/lib.rs (target/debug/deps/first-1234)\n"
                      "suite::single: test\n", status="timeout")


class ListingDeadlineTests(unittest.TestCase):
    def test_expired_budget_does_not_spawn_a_child(self):
        with patch.object(capture_listings.subprocess, "Popen") as child:
            status, text = capture_listings.run(["cargo", "test"], {}, time.monotonic() - 1)
        self.assertEqual(status, "not-run")
        self.assertFalse(text)
        child.assert_not_called()

    def test_deadline_stops_starting_more_configurations_and_checkpoints_missing_sections(self):
        with tempfile.TemporaryDirectory() as temp, \
                patch.object(capture_listings.ci_steps, "refuse_diverged_workflow", return_value=False), \
                patch.dict("os.environ", {"UNIT_TEST_FEATURES": FEATURES, "GITHUB_SHA": COMMIT,
                                          "GITHUB_RUN_ID": "21", "GITHUB_RUN_ATTEMPT": "1"}), \
                patch.object(capture_listings.subprocess, "run", return_value=subprocess.CompletedProcess(
                    ["git", "rev-parse", "HEAD"], 0, stdout=COMMIT + "\n")), \
                patch.object(capture_listings, "build_identity", return_value={"rustc": "rustc test", "cargo": "cargo test"}), \
                patch.object(capture_listings, "run", return_value=("deadline", "interrupted")) as run:
            root = Path(temp)
            self.assertEqual(capture_listings.main(["--out", temp, "--deadline-epoch", str(int(time.time()) + 60)]), 1)
            ledger = read_json(root / "listing.json")
            self.assertEqual({key: ledger.get(key) for key in RUN}, RUN)
            counts = ledger["captures"]
            self.assertEqual(sum("deadline" in values for values in counts.values()), 1)
            self.assertTrue(all("not-run" in values for values in list(counts.values())[1:]))
            self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
