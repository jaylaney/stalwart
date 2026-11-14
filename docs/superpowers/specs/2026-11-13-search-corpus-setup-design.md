# Faster differential corpus setup

The follow-up to PR #84 reduces harness overhead and makes long preparation visible.
It does not change the search implementation or its qualification contract. Base:
`9b1df344b96c0ec808ef3243ed18cb2a209a9089`.

## Construction

The differential fixture submits 3,258 paired messages serially. Between accepted
pairs it drains persisted tasks and search partitions, SMTP queue rows, and tasks
and search partitions again. These barriers keep the ordinary threading oracle
in delivery order. Retain all three scans, their order, skip flags, and deadlines.
A queue notification or timestamp-use counter is not a durable completion proof.

Extract interval-parameterized versions of the existing test helpers, retaining
200 ms task and 100 ms SMTP defaults for existing callers. Corpus setup alone
selects 10 ms polling. Check before sleeping; keep task waits unbounded and the
SMTP-only ten-second deadline. Pending-work diagnostic output remains roughly
once per two seconds, independent of polling cadence. No production scheduling,
notification receiver, request timeout, or global setting changes.

For each keyed stored-MIME identification batch, unlock once and borrow those keys
for the same authenticated per-document stub and full blob reads. Drop keys before
the next delivery batch. Keep the fresh-unlock helper for every existing caller,
including direct headerless checks and lifecycle cases. Derive the account ID
from the keys or verify that an explicit ID matches. Do not cache payloads.

Emit fixed-label setup phases, completed-pair counts, batch counts, elapsed time,
and a ten-second heartbeat while an await remains pending. Observe one pinned
future without cancellation/restart, new timeouts, background tasks, or store
queries for logging. Return a ready work result in the same poll; ordinary scheduler
cooperation still applies. Do not log credentials,
keys, raw MIME, headers, Message-IDs, account names, or content-derived hashes.
Keep output bounded to phase/batch transitions and one active heartbeat.

## Preserved qualification

- Retain all 3,240 base and 18 generated inputs, byte-for-byte resource handling,
  session/chunk boundaries, and serial delivery order.
- Retain paired LMTP acceptance, timestamp guard lifetime, and every barrier.
  Ordinals 292, 555 and 595 still require both refusals and both original-byte
  IMAP APPENDs with equal INTERNALDATE and all current UID/inventory assertions.
- Retain stored metadata authentication, full stored-MIME decryption and hash
  equality, suffix matching including duplicate and nested inputs, exact counts,
  seen-ID checks, and later independent JMAP identity corroboration.
- Retain bulk indexing/currentness, page size 400, full inventories, thread
  partitions with only the existing K deviation, all 183 comparison forms, G5,
  teardown, canonical inclusion, and final store-empty checks.
- Keep production code, corpus resources, query assertions, CI selection, test
  skips, deadlines, build profiles, and environment defaults unchanged.

## Measurement and acceptance

Run the unchanged full canonical suite and the candidate on the same host and
profile, serially with no competing builds. Timestamp output externally. Compare
`differential: paired corpus` to `begin phase=currentness mode=key` on both; this
includes account preparation, corpus loading/delivery/readback, and bulk indexing.
Do not invent baseline subphase timings. Candidate phase timing diagnoses remaining
cost. Randomized generated Message-IDs prohibit an across-run byte digest identity
claim; within-run paired MIME checks remain binding.

Require the final candidate's full `za::za_tests` in default RocksDB blob mode and
`BLOB_STORE=FileSystem`, formatting, affected-target compilation, and independent
whole-diff review. A single A/B observation is not a stable benchmark or a hosted
CI prediction. Preserve logs, binary/source identity, commands, and receipts in
the ignored task ledger. Publish a separate follow-up PR, stacked on PR #84 while
it remains unmerged.
