# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Server-free checks for the shipping cold search runner."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, call, patch

import cold_search as cold


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory()
        self.addCleanup(self.work.cleanup)
        self.directory = Path(self.work.name)
        self.messages = [
            b"Subject: fixture\r\n\r\nbodycanary " + bytes([index])
            for index in range(cold.SMOKE_COUNT)
        ]
        self.manifest = {
            "version": 1, "seed": 1, "count": cold.SMOKE_COUNT, "body_word": cold.BODY_WORD,
            "corpus_sha256": hashlib.sha256(b"".join(self.messages)).hexdigest(),
            "messages": [],
        }
        for index, raw in enumerate(self.messages):
            name = f"{index:06}.eml"
            (self.directory / name).write_bytes(raw)
            self.manifest["messages"].append(
                {"file": name, "sha256": hashlib.sha256(raw).hexdigest(),
                 "bytes": len(raw), "attachment_bytes": 0}
            )

    def load(self, count=cold.SMOKE_COUNT):
        (self.directory / "manifest.json").write_text(json.dumps(self.manifest), encoding="utf-8")
        return cold.Corpus.verify(self.directory, count)

    def test_verified_corpus_is_rechecked_when_submitted(self):
        corpus = self.load()
        self.assertEqual(corpus.sha256, self.manifest["corpus_sha256"])
        self.assertEqual(corpus.read(0), self.messages[0])
        (self.directory / "000000.eml").write_bytes(b"tampered")
        with self.assertRaisesRegex(cold.GateError, "MIME"):
            corpus.read(0)

    def test_full_gate_rejects_smoke_input(self):
        with self.assertRaisesRegex(cold.GateError, "manifest"):
            self.load(cold.FULL_COUNT)

    def test_reordered_entries_and_wrong_types_are_rejected(self):
        original = self.manifest["messages"][:]
        self.manifest["messages"][:2] = original[1], original[0]
        with self.assertRaisesRegex(cold.GateError, "manifest"):
            self.load()
        self.manifest["messages"] = original
        self.manifest["version"] = True
        with self.assertRaisesRegex(cold.GateError, "manifest"):
            self.load()

    def test_incorrect_message_and_aggregate_digests_are_rejected(self):
        first = self.manifest["messages"][0]
        digest, first["sha256"] = first["sha256"], "0" * 64
        with self.assertRaisesRegex(cold.GateError, "MIME"):
            self.load()
        first["sha256"] = digest
        self.manifest["corpus_sha256"] = "0" * 64
        with self.assertRaisesRegex(cold.GateError, "corpus"):
            self.load()


def query_result(ids=("id1",), state="state1", position=0, total=None):
    result = {"accountId": "account", "ids": list(ids), "queryState": state, "position": position}
    if total is not None:
        result["total"] = total
    return result


class ProbeTests(unittest.TestCase):
    def test_setup_limit_one_must_match_first_filtered_census_id(self):
        full = query_result(("first", "second"), total=2)
        jmap = Mock(side_effect=[query_result(("stray",)), full, full])
        receipt = {"setup_search_refusals": 0}
        with self.assertRaisesRegex(cold.GateError, "limit-1"):
            cold.Probe(jmap).prepare(2, receipt, lambda: None)
        self.assertEqual(jmap.call_count, 3)

    def test_transport_failure_still_sends_repeat_and_retains_both_receipts(self):
        jmap = Mock(side_effect=[TimeoutError("do not record credentials"), query_result()])
        probe = cold.Probe(jmap)
        outcomes = []
        probe.measure_pair(outcomes)
        self.assertEqual(jmap.call_args_list, [call("Email/query", cold.QUERY, timeout=600)] * 2)
        self.assertEqual([result["status"] for result in outcomes], ["transport-error", "ok"])
        self.assertEqual(outcomes[1]["ids"], ["id1"])
        self.assertNotIn("credentials", json.dumps(outcomes))

    def test_jmap_refusal_and_malformed_repeat_have_separate_receipts(self):
        jmap = Mock(side_effect=[cold.WireError("search-unavailable"), {"ids": ["other"]}])
        outcomes = []
        cold.Probe(jmap).measure_pair(outcomes)
        self.assertEqual([result["status"] for result in outcomes],
                         ["search-unavailable", "invalid-response"])
        self.assertEqual(jmap.call_count, 2)

    def test_public_census_rejects_duplicate_id_and_unstable_state(self):
        for result in (query_result(("id1",), position=1, total=2),
                       query_result(("id2",), state="changed", position=1, total=2)):
            with self.subTest(result=result), patch.object(cold, "PAGE_SIZE", 1):
                jmap = Mock(side_effect=[query_result(total=2), result])
                with self.assertRaises(cold.GateError):
                    cold.Probe(jmap).census({}, 2)


class ProvisioningTests(unittest.TestCase):
    def test_uses_numeric_duration_and_includes_unbuffered_startup_events(self):
        server = cold.ProductServer.__new__(cold.ProductServer)
        server.objects = Mock(side_effect=lambda kind: {"Tracer": [{"id": "tracer"}]}.get(kind, []))
        server.create = Mock()
        server.update = Mock()
        with patch.object(cold.runner.ProductServer, "provision"), patch.object(
            cold.runner, "port", return_value=17318
        ):
            server.provision()
        throttle = next(args[1] for args, _ in server.create.call_args_list
                        if args[0] == "MtaInboundThrottle")
        self.assertIs(throttle["enable"], False)
        self.assertEqual(throttle["rate"], {"count": 1, "period": 1000})
        kind, object_id, patch_values = server.update.call_args.args
        self.assertEqual((kind, object_id), ("Tracer", "tracer"))
        self.assertEqual(patch_values["eventsPolicy"], "include")
        self.assertIs(patch_values["buffered"], False)
        self.assertIs(patch_values["events"]["server.startup"], True)
        self.assertIs(patch_values["events"]["registry.build-warning"], True)


class DeliveryTests(unittest.TestCase):
    def test_generic_lmtp_success_is_not_counted_as_a_queue_commit(self):
        client = Mock()
        client.ehlo.return_value = (250, b"hello")
        client.has_extn.return_value = True
        client.mail.return_value = (250, b"accepted")
        client.rcpt.return_value = (250, b"accepted")
        client.getreply.return_value = (250, b"2.0.0 Message queued for delivery.")
        context = Mock()
        context.__enter__ = Mock(return_value=client)
        context.__exit__ = Mock(return_value=None)
        corpus = Mock(count=1)
        corpus.read.return_value = b"raw"
        receipt = {"queued": 0}
        with patch.object(cold.smtplib, "LMTP", return_value=context):
            with self.assertRaisesRegex(cold.GateError, "queue commit"):
                cold.deliver(corpus, 17318, "local@cold.test", receipt, lambda: None)
        self.assertEqual(receipt["queued"], 0)

    def test_lmtp_commits_exact_bytes_to_queue_and_stops_at_first_refusal(self):
        client = Mock()
        client.ehlo.return_value = (250, b"hello")
        client.has_extn.return_value = True
        client.mail.return_value = (250, b"accepted")
        client.rcpt.return_value = (250, b"accepted")
        client.getreply.side_effect = [
            (250, b"2.0.0 Message queued with id f01."), (451, b"private detail"),
        ]
        context = Mock()
        context.__enter__ = Mock(return_value=client)
        context.__exit__ = Mock(return_value=None)
        corpus = Mock()
        corpus.count = 5
        corpus.read.side_effect = [b"raw\r\n", b".no terminating CRLF"]
        receipt = {"queued": 0}
        with patch.object(cold.smtplib, "LMTP", return_value=context) as connect:
            with self.assertRaisesRegex(cold.GateError, "451"):
                cold.deliver(corpus, 17318, "local@cold.test", receipt, lambda: None)
        self.assertEqual(connect.call_args.args[:2], ("127.0.0.1", 17318))
        self.assertEqual(receipt["queued"], 1)
        self.assertEqual(client.send.call_args_list, [
            call(b"raw\r\n"), call(b".no terminating CRLF"),
        ])
        self.assertEqual(client.mail.call_args_list, [call("")] * 2)


class LifecycleTests(unittest.TestCase):
    def test_lifecycle_has_no_mail_call_between_restart_and_pair_and_preserves_failure(self):
        server = Mock()
        probe = Mock()
        probe.prepare.return_value = {"ids": ["id1"], "query_state": "state1"}
        events = Mock()
        events.attach_mock(server, "server")
        events.attach_mock(probe, "probe")
        probe.measure_pair.side_effect = lambda outcomes: outcomes.extend([
            {"status": "search-unavailable"}, {"status": "ok", "ids": ["id1"]},
        ])
        receipt = {"pair": []}
        save = Mock()
        with self.assertRaisesRegex(cold.GateError, "pair"):
            cold.restart_and_measure(server, probe, probe.prepare.return_value, receipt, save)
        self.assertEqual([event[0] for event in events.mock_calls],
                         ["server.stop_clean", "server.start", "probe.measure_pair"])
        self.assertEqual(server.start.call_args, call(recovery=False))
        self.assertEqual(len(receipt["pair"]), 2)
        save.assert_called()


if __name__ == "__main__":
    unittest.main()
