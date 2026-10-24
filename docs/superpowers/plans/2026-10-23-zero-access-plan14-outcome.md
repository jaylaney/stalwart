# Calendar sharing implementation and qualification outcome

Dated 2026-10-24. Plan 14, revision 14 sharing design. Implementation/Task 11 head:
`e88b5c1e16487e7bcc385f952f9162ecc43b49ba`; implementation commits begin after `cd48a8f1` (the accepted plan/interface
clarification). Task 12 changes only the six documentation files named by its brief.
No production source, persisted struct layout, dependency or generated schema changed here.

## Status and evidence boundary

The complete final automated net, including the shipping release build, passed sequentially
on the exactly restored implementation. The release build finished successfully in 1145.87 s.
The 3,000-event first-grant and interrupted/resumed measurement completed with full Alice/Bob
readback and exact temporary-source restoration. The scanner/mutation qualification is
recorded in the committed [Task 11 evidence](2026-10-23-zero-access-plan14-mutations.md).
**Fresh whole-branch review is pending**, owned by the primary with a clean-context reviewer.
**Native qualification is pending** for macOS Apple Calendar, iOS Apple Calendar and
Thunderbird. This outcome therefore does not claim complete release or native readiness.
No SQL backend, multi-node cluster, production deployment or real-account test was run.

Task 12 scoped independent documentation review (`gpt-6-sol`, high) approved the six-file
freeze with 0 Critical, 0 Important and 1 Minor: inherited warning disclosure remains for
the relevant source owner. Raw benchmark/seven completed gate receipts and durable patch
restoration were independently checked. This scoped approval leaves fresh whole-branch review and native qualification pending.

Tasks 1–10 implemented collection-key formats/cache, codecs, fresh resolver authority,
event/link fences, conversion/rollback, endpoints, delegated scheduling and DAV lifecycle.
Their commits are respectively `5917c641`, `b6d86088`, `272abc84`, `f11c701d`, `ce038a05`,
`fde46b4d`, `a214e886`, `8b9d467f`, `b0544562`, `e58595e5`; Task 11 is `e88b5c1e`.
Earlier per-task results remain implementation-phase evidence, not substitutes for this net.

## Implemented contracts

| Invariant | Implemented source and acceptance evidence |
|---|---|
| 33: plaintext CK residency/domain cache | `crates/vault/src/session.rs`, `crates/groupware/src/calendar/sharing/format.rs`; owner/grant rings and purpose/AAD/domain checks; format units and shared healthy/scanner controls |
| 34: principal-bound CK wraps | `crates/groupware/src/calendar/sharing/format.rs` and `resolve.rs`; owner EWK and pinned-grantee public-key wraps bind exact principal/calendar/domain/AAD, and whole-ring cache identity prevents partial reuse; format/codec units and negative reader controls |
| 35: controlled grant authority | `crates/http/src/api/shares.rs`, `calendar/sharing/mutations.rs`; primary verification, DavCalAcl, recorded pins, physical calendar/vault/Account/pin assertions; HTTP races and qualified mutations |
| 36: sole parent and current-domain writes | `calendar/sharing/convert.rs`, DAV event writers, drain/contacts integration; phase-1 parentage walk, rollback, original event CAS and bounded rebuilds; mixed 3,000-row measurement and race suite |
| 37: future-write rotation | `calendar/sharing/mutations.rs`, `calendar/seal/domain.rs`; rotation retains old generations, removes target authority, current generation used on new writes; revoke/regrant/race coverage |
| 38: delegated scheduling | `calendar/{delegation,fanout,drain,pinning}.rs`, DAV update/delete and scheduling builders; writer keys/pins versus owner locality/lease identity, retained Self provenance and SENT-BY verification; lifecycle differential cases and mutation qualification |
| 39: no owner session/key transfer | `crates/vault/src/session.rs`, `calendar/sharing/resolve.rs`; grantee's own session/ring and whole-ring cache identity; wrong-domain/cache identity units and refusal cases |
| 40: own-id permission | DAV gate and `calendar/sharing/resolve.rs`; fresh own-id ACL bits and grant ring, impersonation/group/member shortcuts refused; admission/discovery/race tests and qualified row 19 own-id oracle |

The accepted implementation interpretations are preserved below. Scanner checks are
structural checks of visible carriers, parentage and correspondence; they do not authenticate
CKs, encrypted ledgers or associated data. Runtime readers/codecs provide separate crypto
checks. Native UI behavior remains a separate gate.

## Final automated verification

Every Cargo command used `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`; stable Rust
1.99.0/Cargo 1.99.0. All integration suites ran sequentially with disposable reset state.
The table reports actual test-result lines, including doc tests where emitted, and total
command time. Empty doc-test counts are not substituted for the nonzero unit/suite counts.

| Gate | Exact acceptance command | Actual result | Command time |
|---|---|---|---|
| final-format | `cargo fmt --all --check` | exit 0 | 3.39 s |
| final-units | `cargo test -p vault -p groupware -p common` | common 121, groupware 146, vault 45 passed; 0 failed; doc tests 0/0 | 9.60 s |
| final-http-units | `cargo test -p http@0.16.26 --features test_mode` | 6 passed, 0 failed; doc tests 0/0 | 29.01 s |
| final-za | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture` | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 354.82s | 392.59 s |
| final-webdav | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests` | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 16.96s | 17.87 s |
| final-key-webdav | `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests` | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 21.30s | 21.81 s |
| final-key-jmap | `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests` | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 142.03s | 142.57 s |
| final-product-build | `cargo build --release -p stalwart --no-default-features --features rocks` | exit 0; release profile finished | 1145.87 s |

The key-mode JMAP run set `RUST_TEST_NOCAPTURE=1` to expose the existing
`Running key-account Calendar tests...` marker without changing its Cargo arguments.
`tests/src/jmap/mod.rs` selects `za_variants::calendars` in this mode; that executed module
asserts seven Calendar/CalendarEvent/CalendarEventNotification/ParticipantIdentity methods
return `accountNotSupportedByMethod`, empty Principal availability, and empty cleanup state.
The complete selected suite passed after that marker. No final-net retry was required.

The benchmark had one separate setup-only launch failure: STORE/RUST_MIN_STACK were omitted
from the invocation; the harness refused at utils/server.rs:109 before creating its server.
Its failed log/exit101 was retained. The explicitly corrected invocation ran once. This is
not a behavioral failure or a successful measurement. The temporary fixture first compiled
successfully with `cargo test -p tests za::za_tests --no-run`.

Warnings were inspected and retained; no new production warning was introduced because
production source was unchanged. The observed warning summary is:

```text
final-units: warning: unused import: `LookupStore`
final-units: warning: `store` (lib) generated 1 warning (run `cargo fix --lib -p store` to apply 1 suggestion)
final-units: warning: unused imports: `registry::RegistryQuery` and `roaring::RoaringBitmap`
final-units: warning: unused import: `MaskedEmail`
final-units: warning: unused import: `now`
final-units: warning: unused import: `structs::MaskedEmail`
final-units: warning: unused import: `types::id::Id`
final-units: warning: unused import: `compiler::Number`
final-units: warning: unused import: `std::time::Instant`
final-units: warning: unused imports: `AiEvent` and `SecurityEvent`
final-units: warning: variable does not need to be mutable
final-units: warning: unused variable: `tenant_id`
final-units: warning: unused variable: `domain_name`
final-units: warning: unused variable: `issuer`
final-units: warning: unused variable: `domain`
final-units: warning: unused variable: `storage`
final-units: warning: unused variable: `ctx`
final-units: warning: unused variable: `is_enterprise`
final-units: warning: field `data` is never read
final-units: warning: `common` (lib) generated 18 warnings (run `cargo fix --lib -p common` to apply 17 suggestions)
final-units: warning: `common` (lib test) generated 18 warnings (18 duplicates)
final-http-units: warning: unused import: `LookupStore`
final-http-units: warning: `store` (lib) generated 1 warning (run `cargo fix --lib -p store` to apply 1 suggestion)
final-http-units: warning: unused imports: `registry::RegistryQuery` and `roaring::RoaringBitmap`
final-http-units: warning: unused import: `MaskedEmail`
final-http-units: warning: unused import: `now`
final-http-units: warning: unused import: `structs::MaskedEmail`
final-http-units: warning: unused import: `types::id::Id`
final-http-units: warning: unused import: `compiler::Number`
final-http-units: warning: unused import: `std::time::Instant`
final-http-units: warning: unused imports: `AiEvent` and `SecurityEvent`
final-http-units: warning: variable does not need to be mutable
final-http-units: warning: unused variable: `tenant_id`
final-http-units: warning: unused variable: `domain_name`
final-http-units: warning: unused variable: `issuer`
final-http-units: warning: unused variable: `domain`
final-http-units: warning: unused variable: `storage`
final-http-units: warning: unused variable: `ctx`
final-http-units: warning: unused variable: `is_enterprise`
final-http-units: warning: field `data` is never read
final-http-units: warning: `common` (lib) generated 18 warnings (run `cargo fix --lib -p common` to apply 17 suggestions)
final-http-units: warning: unused imports: `ObjectType` and `Property`
final-http-units: warning: unused import: `blob_hash::BlobHash`
final-http-units: warning: `services` (lib) generated 2 warnings (run `cargo fix --lib -p services` to apply 2 suggestions)
final-http-units: warning: unused imports: `RegistryObjectCounter` and `RegistryQuery`
final-http-units: warning: unused import: `ObjectType`
final-http-units: warning: unused import: `RegistryQuery`
final-http-units: warning: `jmap` (lib) generated 3 warnings (run `cargo fix --lib -p jmap` to apply 3 suggestions)
final-za: warning: linker stderr: ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected
final-za: warning: `tests` (lib test) generated 1 warning
final-webdav: warning: linker stderr: ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected
final-webdav: warning: `tests` (lib test) generated 1 warning
final-key-webdav: warning: linker stderr: ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected
final-key-webdav: warning: `tests` (lib test) generated 1 warning
final-key-jmap: warning: linker stderr: ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected
final-key-jmap: warning: `tests` (lib test) generated 1 warning
final-product-build: warning: unused import: `LookupStore`
final-product-build: warning: unused imports: `registry::RegistryQuery` and `roaring::RoaringBitmap`
final-product-build: warning: unused import: `MaskedEmail`
final-product-build: warning: unused import: `now`
final-product-build: warning: unused import: `structs::MaskedEmail`
final-product-build: warning: unused import: `types::id::Id`
final-product-build: warning: unused import: `compiler::Number`
final-product-build: warning: unused import: `std::time::Instant`
final-product-build: warning: unused imports: `AiEvent` and `SecurityEvent`
final-product-build: warning: variable does not need to be mutable
final-product-build: warning: `store` (lib) generated 1 warning (run `cargo fix --lib -p store` to apply 1 suggestion)
final-product-build: warning: unused variable: `tenant_id`
final-product-build: warning: unused variable: `domain_name`
final-product-build: warning: unused variable: `issuer`
final-product-build: warning: unused variable: `domain`
final-product-build: warning: unused variable: `storage`
final-product-build: warning: unused variable: `ctx`
final-product-build: warning: unused variable: `is_enterprise`
final-product-build: warning: field `data` is never read
final-product-build: warning: unused imports: `ObjectType` and `Property`
final-product-build: warning: unused import: `blob_hash::BlobHash`
final-product-build: warning: unused imports: `RegistryObjectCounter` and `RegistryQuery`
final-product-build: warning: unused import: `ObjectType`
final-product-build: warning: unused import: `RegistryQuery`
final-product-build: warning: `common` (lib) generated 18 warnings (run `cargo fix --lib -p common` to apply 17 suggestions)
final-product-build: warning: `jmap` (lib) generated 3 warnings (run `cargo fix --lib -p jmap` to apply 3 suggestions)
final-product-build: warning: `services` (lib) generated 2 warnings (run `cargo fix --lib -p services` to apply 2 suggestions)
```

## Disposable 3,000-event measurement

Each of two independent calendars contained exactly 1,500 EWK-sealed events and 1,500
legacy plaintext events. `plant_event` deliberately bypasses DAV sealing; these are legacy
rows, not newly created ordinary key-account DAV events. Each small single-VEVENT fixture
has a unique UID and fixed summary, with no attendee, alarm or recurrence. The original separate all-plaintext
3,000-row budget fixture remains unchanged. Timers enclose an actual POST grant through
complete JSON response consumption, excluding setup and verification reads.
The full and resumed successful requests have no induced pause. The failed request has two
one-shot existing pause points: after phase 1, then after exactly 100 acknowledged event
writes/before group 2 writes. FAIL_ONCE is armed at the second pause. This is injected store
failure with a persisted journal, not a process restart or power-loss test.

After the 500, the authenticated journal had converted=false, reverting=false, no grants,
no ACL and one owner preference. Complete raw membership still equalled the original 3,000
ids. The 100 CK rows were partitioned by original input kind, and all remaining rows retained
their original kind. Alice read every body unchanged and Bob remained 403. The new request
reported only its remaining acknowledged work; prior request work was not fabricated from
the 500 body. General counts remain work counts, not distinct ids across rollback/reconversion.

On both final calendars the authenticated owner ring was converted and not reverting, with
exactly Bob's grant and matching ACL; all 3,000 rows were authenticated CK rows with unchanged
names/parent, created/modified, Schedule-Tag and flags. Exact id membership, unique names and
UIDs were checked; all 3,000 Alice GET bodies matched baseline and all 3,000 Bob GET bodies
using Bob's own credentials matched Alice. Verification checks include all rows, not samples.
The whole instrumented ZA suite passed through account destruction and strict final-empty
cleanup in 719.44 s command time (718.89 s test runtime). Fixture setup/baseline reads were
not separately timestamped; no setup-only duration or throughput is inferred from that total. Both temporary calendars were deleted through HTTP and task completion was awaited.

Actual compact measurement receipts (request sum and workflow wall time including checkpoint
reads are deliberately separate metrics):

```json
[
  {
    "calendar": "benchmark-full",
    "attempt": "first-grant",
    "status": 200,
    "response": {
      "ok": true,
      "changed": true,
      "rewrapped": 1500,
      "sealed": 1500
    },
    "request_ms": 2928,
    "sum_http_request_ms": 2928,
    "workflow_wall_ms_including_checkpoint_reads": 2928,
    "initial_ewk_sealed": 1500,
    "initial_legacy_plaintext": 1500,
    "logical_group_limit": 100,
    "physical_event_commit": "one event",
    "in_process_test_server": true,
    "debug_assertions": true
  },
  {
    "calendar": "benchmark-full",
    "verification": "passed",
    "exact_id_inventory": 3000,
    "unique_names": 3000,
    "unique_uids": 3000,
    "alice_exact_body_matches": 3000,
    "bob_own_credential_exact_body_matches": 3000
  },
  {
    "calendar": "benchmark-resume",
    "attempt": "interrupted",
    "status": 500,
    "response": {
      "type": "about:blank",
      "status": 500,
      "title": "Internal Server Error",
      "detail": "There was a problem while processing your request. Please contact the system administrator if this problem persists."
    },
    "request_ms": 192,
    "induced_pause": true
  },
  {
    "calendar": "benchmark-resume",
    "checkpoint": "after500",
    "committed_envelope_rewrites": 50,
    "committed_plaintext_seals": 50,
    "inventory": 3000,
    "converted": false,
    "reverting": false,
    "grants": 0
  },
  {
    "calendar": "benchmark-resume",
    "attempt": "resume",
    "status": 200,
    "response": {
      "ok": true,
      "changed": true,
      "rewrapped": 1450,
      "sealed": 1450
    },
    "request_ms": 3040,
    "sum_http_request_ms": 3232,
    "workflow_wall_ms_including_checkpoint_reads": 6382,
    "initial_ewk_sealed": 1500,
    "initial_legacy_plaintext": 1500,
    "logical_group_limit": 100,
    "physical_event_commit": "one event",
    "in_process_test_server": true,
    "debug_assertions": true
  },
  {
    "calendar": "benchmark-resume",
    "verification": "passed",
    "exact_id_inventory": 3000,
    "unique_names": 3000,
    "unique_uids": 3000,
    "alice_exact_body_matches": 3000,
    "bob_own_credential_exact_body_matches": 3000
  }
]
```

Machine/build/conditions: Mac13,1, 10 physical/logical CPUs, 64 GiB RAM, Darwin 27.0.0 arm64,
macOS 27.0.1 build 26A434. rustc 1.99.0 (b940084d7, 2026-09-28), Cargo1.99.0.
This is the in-process test server over self-signed loopback TLS, RocksDB, profile.test
opt-level=0/debug=1/incremental/debug assertions (Argon2 separately opt-level=3), default
rocks+sqlite test features and upstream test_mode/enterprise test dependencies. Test-mode
Argon2 parameters are weaker than shipping parameters. Seed/baseline reads warm owner paths
and storage; the normal suite task consumer and drain-skip default remain active. Request
client timeout 180 s; no concurrent build/suite was started by this executor. No NO_INSERT,
.run state or real account was used. **This is not shipping release-profile throughput.**
The separate shipping release build passed the build gate; these measurement results still
come from the in-process test profile. No hostname, login name,
serial, UUID, credential or key material is needed in this report.

Measured source was frozen after formatting. Original sharing_races.rs SHA256:
`50004e1c6104a550c21cf1ef5c327713fac5aa7950fc45189afba1f6b33eb9aa`. The exact applied patch SHA256 is
`050ec1f1bf53f2e9278288743ab601cb0552f16cb57b294792609d008be7d42b`; Cargo.lock SHA256 `073928a7fc7e407b383217bb384f42118528ab080491247ad9d7d897f0cd8a39`.
Restoration copied exact saved bytes, without reset/clean/checkout. The restored SHA equals
its original and the prepatch status baseline returned exactly before the final net.
All sharing hooks use existing drop guards; no hook or production-source change remains.
The complete exact temporary instrumentation is preserved in Appendix C as newline-preserving
JSON strings, so the evidence survives ignored-workspace cleanup.

## Task 11 scanner and mutation qualification

Committed Task 11 evidence qualifies all 57 required rows after the formal review corrections:
85 executed attempts comprise69 valid kills (including row 7's supplementary attempt 2),
11 inconclusive,4 survived and1 invalid/superseded qualification (19/3). Prepared 5/1 and
compile-interrupted 6/1 carry no execution result. The original all 57 statement was premature.
Corrected 19/4 detects an actual forbidden assisted-discovery href under M6; corrected 48/2
covers case 10 finalize, alongside the original case 25 half. Request 500 **or** exact item 500
is accepted for corrupt eligible data; stronger whole-request-only packaging was rejected.

The structural scanner has 23 corruption/restoration controls, including malformed archive,
distinct parents, CK-on-unshared, and healthy multigeneration/personal/Apple-property controls.
Fresh restored Task11 gates were ZA 1/0 in 357.27 s, common 121/groupware 146/vault 45, format and
whitespace checks; the scoped independent rereview approved all 3 corrections with 0 new
findings. Historical results apply to their named hashes; this Task12 net is separately fresh.
M6 rows 25/32 omit every physical copy of the same path rule; they do not show that one
isolated hint/fence omission bypasses duplicate defenses. M9 row 13 is positive-grant
preemption by production correspondence checking, not reader-side cryptographic proof.
Scoped required-case runs retain original earlier-case preemptions. Appendix B preserves
all external R11 decisions/statuses; the linked mutation note holds exact patches/hashes.

## Native and remaining gates

| Gate | Status and exact missing evidence |
|---|---|
| Fresh whole-branch review | Pending primary dispatch of a clean-context read-only reviewer across combined authorization, key identity, ordering, persistence, conversion/revoke fences, boundaries and scheduling; then adjudication/affected rechecks |
| Apple Calendar macOS | Pending disposable-account/client observations for assisted discovery, view/edit, colour/order, invite/SENT-BY, own/shared replies, sole-attendee decline, display alarm, drag requests/results, revoke/regrant and rename; this Mac host has no recorded qualified disposable native session/trusted-TLS setup or these client observations |
| Apple Calendar iOS | Pending the same observations on a physical iOS client; device/client observations, qualified reachable-TLS setup and disposable native credentials remain unexercised |
| Thunderbird | Pending assisted-discovery-off second-home-href discovery and shared access on an actual client |
| DAVx5 | Optional; not run |
| Circulo | Sharing UI and its separate docs/api.md update pending separate repository work |
| Shipping release build | Passed; prescribed no-default-features rocks command, exit 0, 1145.87 s |
| Deployment/release | Not performed; requires separate authorization |

Record immediate UI and post-drain/refresh state separately on owner and grantee. Automated
success, persisted state and HTTP results do not establish those native observations.

## Accepted behavior and limitations

The following seven scheduling behaviors are inherited and accepted by revision14 §6.2;
they are not silently normalized away by the fork's parity oracle:

| Behavior | Source/observed design mechanism | Accepted disposition |
|---|---|---|
| 1. A later full REQUEST reactivates an override an instance CANCEL had cancelled (re-review finding 1, revision 9) | The full REQUEST carries every component with attendees to every attendee of any component (`organizer.rs:325-329`, `:366-373`); the CANCEL merge removes SEQUENCE and STATUS and adds back `STATUS:CANCELLED` without a SEQUENCE (`inbound.rs:198-230`); the REQUEST merge accepts `>=` against the absent SEQUENCE and replaces STATUS (`inbound.rs:101-133`; STATUS is a snapshot entry, `snapshot.rs:299`). If the override lost its last attendee the REQUEST omits it and the merge removes it (`inbound.rs:157-166`): active either way. The plain inbox path uses the same merge (`crates/groupware/src/calendar/itip.rs:204-212`, `:788`) as the drain (`drain.rs:1075-1081`, `:1099`). | Inherited; accepted, not fixed. |
| 2. Two addresses of one account are two attendees to the builders and one copy to the drain | Removing alias A from the master while alias B attends an override emits the full REQUEST (to B among others) and a partial CANCEL of the main instance to A, in that order; the single copy (`drain.rs:702-714`) applies the REQUEST, then the master CANCEL cancels the series, the override included (`inbound.rs:231-232`, `:265-305`). The fork's delivery keeps the emitted order (items written in message order, `fanout.rs:455-549`, applied in id order, `drain.rs:417-423`), so the outcome is deterministic; upstream's own plain path is not (the queue shuffles scans of more than three messages, `crates/smtp/src/queue/manager.rs:157-159`). | Inherited; accepted. Revision 9's alias rules (the whole-event payload per alias, the account-level CANCEL) are withdrawn by this revision; revision 6's one self item per address stands. |
| 3. An override-only writer's copy stays active after the event's deletion (re-review clarification, revision 9) | `itip_cancel` collects recipients over every component (`event_cancel.rs:41-52`) but builds a master-only CANCEL with the master's ATTENDEE lines (`:55-67`, `:164-165`); the writer's drain finds no local line and refuses it (`snapshot.rs:353-361`; `drain.rs:584-589`), as a plain account's ingest does (`crates/groupware/src/calendar/itip.rs:157`). | Inherited; accepted: answered by parity, with no component retained in the CANCEL. |
| 4. A group address both accounts hold gets no self-message | The address is local under the owner-only list too (6.2's gate), as it is for the owner's own write. | Inherited; accepted. |
| 5. A REPLY as the owner when the writer also attends (the limits below) | `local_attendee()`, the first local line of a hash set under the merged list (below). | Inherited; accepted. The second run is skipped there (6.2's gate). |
| 6. A message whose payload has no listed, server-scheduled line of the recipient can reach its inbox but fail incoming validation | `Email::new` uses exact local-address membership (`scheduling/mod.rs:244-250`); the incoming snapshot requires some local server-scheduled line (`snapshot.rs:217`, `:353-361`). Account resolution for delivery does not change that snapshot rule. | Inherited; accepted in both runs. Case 25 separately checks transport parity and receiver rejection; no self-only validation exception. |
| 7. A shared owner/writer group as ORGANIZER enters the delegated-organizer path, then its item is refused at the recipient's drain | The shared group is owner-local for the gate and writer-owned for `SENT-BY`, so the fan-out adds no `SENT-BY`; the named group resolves to the group id rather than the writer's `changed_by`, and the ordinary sender check rejects it. | Inherited; accepted. Case 25 checks the gate, omitted `SENT-BY` and receiver rejection. |

Other retained boundaries: MOVE overwrite preserves the upstream no-CANCEL outer result;
UID collision across owner calendars is retained; owner-versus-writer attendee REPLY choice
can follow upstream's merged local-line order; per-component SENT-BY lifecycle is preserved.
The documented fork limitation is attendee-side REPLY delivery to an unlisted owner
sub-address: it may reach the owner's inbox where owner-written parity would yield 5.3.
No broad routing change was authorized. A delegated 3.8 refusal was decided under the
writer's pins; the owner's contacts page cannot redeliver it, and a later event edit must
construct a new message to try that recipient again. An attendee who is also a grantee
should respond from their own invitation copy.

Shared format clears an admitted legacy JMAP-only CalendarPreferences.color field; actual
DAV Apple colour/order dead properties remain shared and preserved. Personal name,
description, custom TZ, flags/sort/default alerts follow the accepted codec contract.
Revocation rotates future writes without resealing old events: removing someone stops new
fresh authorized use, while data/keys already copied cannot be recalled. A request already
in flight can finish a read from its keys; its conflicting write still loses its fence.
Cache invalidation needs cluster pubsub; missed delivery can leave resident keys until the
existing hard cap even though fresh ACL/ring checks refuse revoked use. Logout does not
promise instant key erasure. Running servers handle plaintext and credentials during use.

The narrow grant exception opens an existing owner private-key carrier only to authenticate
WrapMark when pins are absent/unusable, drops the zeroizing secret before another await and
rereads pins. It does not use that key for sharing, self-check, lazy publication or public-key
repair. Usable-pin grants and all revokes avoid that path. List can perform the pre-existing
private-key self-check/public-key repair while opening shared-with-me. New sharing telemetry
excludes bodies, addresses, fingerprints, content and keys; exact inherited authentication
AccountName fields remain a field-specific exception, not a blanket auth-event filter.

At maxShares, gone ledger records count before mutation; revoke/sweep then retry may require
one extra owner request. Address-revoke selects the distinct-id union of all exact recorded
matches plus a resolved current grant; ambiguous aliases require stable-id revoke. Counts
include actual acknowledged rollback/reconversion work, possibly twice for one id; `changed`
includes acknowledged ring-only writes by this request.

## Appendix A: accepted implementation interpretations and decision ledger

The accepted contract below is preserved as an implementation interpretation, not an
independent review conclusion. Subsequent whole-branch adjudications must update its status.


### Current implementation decisions

These are adopted implementation interpretations, subject to the pending final whole-branch
review. The cost column preserves recorded costs; “not separately stated” does not mean
there is no risk. Process/context entries are not counted as additional design decisions.

| Decision | Why and accepted behavior | Recorded cost/status |
|---|---|---|
| Worktree and commit ownership | Reuse the existing isolated checkout; primary integrates only after review | Current process; reversible branch management; agents make no Git writes |
| C1 boundary completion | Own grant/rename loss rereads archive then change id and retries once; opportunistic repair may leave pending after one loss | Current; conservative extra full sync if wrong |
| T1-G generation exhaustion | checked_add refuses rotation beyond u32::MAX without changing valid stored generations | Current; small removable utility |
| T2-MOVE | Validate source envelope/sole parent, rewrap, then change names; plaintext full-seal can receive destination names first | Current; no read-time relaxation; separate cost not stated |
| T2-P domain principal | Domain binds the authenticated principal for personal views; owner-only bundle preparation is explicit | Current; no persisted/wire change; separate cost not stated |
| T3-F fixtures | Private domain construction requires a test_mode-only sealed-calendar builder | Current test plumbing; removable utility, no shipping authority API |
| T3-I identity | Reject token/session-key id mismatch and impersonated foreign use; owner with own keys remains admitted | Current; no widening of za_keys_for; separate cost not stated |
| T3-N missing calendar | Preserve precise NotFound/404/omission and retain all other errors | Current; no new error variant; separate cost not stated |
| T3-C cache witness | Atomically bind the complete cached secret set to successful authentication of exact fresh whole-ring bytes | Current; session-only metadata, no permission authority; separate cost not stated |
| T3-B pending flags | Owner pending state aggregates structurally validated grant-header flags | Current; explicit repair later, no resolver mutation; separate cost not stated |
| T4-T timestamps | Preserve one initial created/modified timestamp through bounded retries and return actual committed ETag | Current; narrow storage writer variants, no general batch API; separate cost not stated |
| T4-L second create loss | Opaque second loss releases the fenced lease before bounded retake; false release is inconclusive | Current; no successor clearing, extra retry or fan-out replay; separate cost not stated |
| T5-R repair | Resolution stays read-only; explicit DAV mutation adapters use the repair-returned fresh context | Current; narrow adapter scope, pending retained on repair loss; separate cost not stated |
| T6-C commit granularity | Split BatchBuilder errors obscure prefixes, so commit each fenced event and count after acknowledgement | Current; at most 100 payloads per logical group, more physical commits; no separate cost statement |
| T5-C collection copy | Fence create/overwrite then fresh-read destination and fence every child write; never replay creation | Current; previously committed prefixes may remain on later conflict |
| T7-M WrapMark exception | Absent/unusable pins require authenticated mark classification; unwrap transiently, drop before await and reread pins | Current explicit exception to old “never open” wording; no sharing use/self-check/repair; no new primitive/layout |
| T5-D MOVE domain | Reverting shared CK differs from unshared EWK even when effective write mode matches | Current; actual cryptographic domain identity, no separate cost stated |
| T5-I1 off-path parents | Keep raw archive fences without requiring keys/authority for surviving private links | Current; preserves explicit unlink recovery; separate cost not stated |
| T5-V visibility | A narrow pub(super) bridge supports test_mode MOVE coverage without public shipping authority | Current; small removable internal visibility change |
| T6-N work counts | Count each acknowledged envelope rewrite/full seal, including rollback then forward work on one id | Current; count-semantics refinement if wrong, no crypto/persistence change |
| T6-P legacy colour | Shared format excludes old JMAP-only personal colour; DAV Apple colour/order stay preserved | Current; rare legacy JMAP preference loss; any preservation needs explicit format/migration decision |
| T6-U original event CAS | Conversion rewriting the held event makes its original CAS return 409; calendar-only change may rebuild once | Current; client optimistic retry if wrong; relaxing CAS needs a separate design |
| T6-M memory bound | Walk retains ids, each group at most 100 payload snapshots, final validation streams archives | Current; O(N ids)+100 payloads; no store API change |
| T7-H HTTP pauses | Standalone HTTP test_mode lacks common/test_mode, requiring two existing-pattern no-op production forwarders | Current test seams; two small internal wrappers/test overhead |
| T7-G gone ledger cap | Current ledger including gone entries counts before conversion/sweep; revoke/sweep then retry | Current; one extra cleanup request at capacity |
| T7-A address revoke | Distinct-id union of all ledger matches and resolved current id can be ambiguous | Current; stable-id revoke required for ambiguous aliases |
| T7-W changed witness | Track this request's acknowledged ring-only conversion writes as well as event work | Current; small internal signature plumbing, no stored or global API change |
| T9-R recipient count | Expose only read-only recipient count for combined DAV limits; retain opaque provenance/payload | Current; small removable accessor |
| T10-G gate order | Preserve ordinary classification errors and owner-key admission before foreign restrictions | Current; keyed owner still sees existing registry lookup failure |

### Temporary qualification decisions

These authorize test/mutation construction only. Every temporary production mutation was
restored; they introduce no shipped behavior. Exact historical patches and current status
remain in the Task 11 note and Appendix B.

| Decision | Reason and permitted scope | Cost/current disposition |
|---|---|---|
| M6 physical copies | Remove every copy of one logical path rule to qualify that rule, preserving unrelated guards/operations | Current; repeat affected qualification if wrong; rows 6/19/25/32 have explicit duplicate-defense limits |
| M2 revoke invalidation | Keep prerequisite grants functional so the required post-revoke cache assertion executes | Current; repeat row 2 if wrong |
| M9 case-3 omission | Scope the same conversion omission to the prescribed existing fixture while preserving other cases/guards | Current; repeat row 9 if wrong; no new permanent hook/API/crypto |
| R11-7a/7b | Supplement case 21 with grantee and creates-only conditions, adding explicit final CK structure | Current qualified evidence; supplementary status retained |
| R11-10/13/13b | Correspondence/positive-grant refusal can preempt the prescribed reader; isolate the required fixture where needed | Current M9 positive-path qualification; not reader-side cryptographic proof |
| R11-11/15 | Surviving initial mutants required generation and downgrade-race oracles | Current; original survived attempts preserved; separate cost not stated |
| R11-19b | Whole-request 500 was incorrectly required instead of permitted request/item 500 | Superseded; 19/3 invalid qualification, repeat/correction cost retained |
| R11-19-review | Allow both error forms; remove both assisted eligibility checks under M6 and detect actual forbidden href | Current 19/4 qualified; no stricter error packaging contract |
| R11-22/26/33/37/38/43 | Scope the same physical omission to required-case UID/call paths after earlier-case preemption | Current; extra runs (R11-22 records three); earlier detections preserved |
| R11-25/32 | Exercise logical omission across all physical same-path defenses | Current limited qualification; does not prove one isolated omission bypasses duplicate checks |
| R11-29/31 | Add unlimited/limit-one sync and an unpaused rename; timeout alone is not a kill | Current; original survived/inconclusive attempts retained; separate cost not stated |
| R11-46(a)/(b) | Direct gate oracle detects the rule; attendee-only Schedule-Tag change proves HTTP observability | Current; original/second-run traces retained; wrong oracle would fail baseline and be withdrawn |
| R11-48 | Original case 25 kill lacked sensitive case 10 coverage; add ForceSend source-absence oracle | Historical partial status superseded; both case 25 and new case 10 halves now qualified |
| R11-validation-review | Accept scoped M6/M9/required-case evidence with explicit limits | Current; repeat affected qualification if wrong; no shipped change |

### Accepted implementation contract

This supplements the binding revision14 sharing spec and approved plan with implementation
interpretations. It is a contract for review, not evidence of correctness or passing tests.
It contains no prior reviewer findings. Refresh after subsequent tasks before final review.

- Current authorization uses the authenticated token's own id and matching SessionKeys,
  rejects impersonation, checks all required ACL bits, and reads the current calendar before
  CK cache access. A domain binds the request's principal for personal views and writes.
- Owner resolution freshly authenticates its complete encrypted ring. Grantee cache reuse
  requires exact fresh grant-ring bytes authenticated previously, represented by a whole-ring
  digest bound atomically to the complete cached key set. Cached keys or witness metadata
  alone confer no permission or current-generation authority.
- Missing current calendars preserve NotFound/404 semantics. Corruption and other store
  failures retain their error semantics. Resolution is read-only; DAV owner boundary repair
  is an explicit adapter action using the returned archive for its view and future fences.
- Owner pending state aggregates owner and structurally validated grant headers. Own grant
  or rename completion uses the exact committed archive/change metadata; after one assertion
  loss it rereads archive then account log and retries once. Further loss stays pending.
  Opportunistic read repair makes one attempt and may return pending after its loss.
- Envelope-only MOVE validates the original source envelope and sole parent before rewrap,
  then updates metadata. Domain identity includes calendar and domain id even while reverting
  writes use EWK. A sealed CK source moving to unshared rewraps to EWK.
- Addressed DELETE/overwrite unlink admission does not require keys or extra authority for
  surviving off-path private parents. Fresh raw presence/archive fences still protect those
  parents. Source/destination admission and shared-linked-event refusals apply as specified.
- Every physical write has its required calendar/event/vault/Account/pin assertions. Event
  retries preserve the initial timestamp and original event/UID CAS, scheduling results and
  bounded calendar-only rebuild. A second opaque create loss releases the fenced UID lease
  before the existing bounded retake/outcome path; it does not identify which assertion lost.
- Key-account collection COPY/MOVE uses indexed create/overwrite plus separately fenced
  child writes and fresh destination readback. Every child read/retry rejects a newly shared
  source/destination/other parent before linking. It does not replay creation; inherited
  already-committed prefixes can remain on a later conflict. Original source deletion is last.
- Conversion logical groups contain at most100 events, with individually fenced physical
  event commits and counts updated after each acknowledgement. The full parentage walk
  retains O(N) ids; at most100 archived payload snapshots are retained for a group; final
  validation streams current archives. This avoids unknown partially committed batch counts.
- `rewrapped` counts acknowledged envelope rewrites in this HTTP request, including rollback
  and forward writes to the same event. `sealed` counts acknowledged full legacy plaintext
  seals. Ring-only writes, failed attempts and work by another request do not enter counts.
  `changed` also includes any ring-only conversion write acknowledged by this request.
- A conversion-rewritten event makes a held UPDATE lose its original event CAS and return409.
  A calendar-only held UPDATE before conversion rewrites its event may rebuild and succeed.
  The converter then authenticates and skips that concurrent CK write.
- Shared preference formats clear the legacy JMAP-only CalendarPreferences.color field.
  Actual DAV Apple colour/order dead properties and other supported preference fields remain
  preserved through conversion/rollback. No invented migration or new stored field is added.
- Grant/revoke do not use the owner's private key for sharing or invoke self-check/lazy
  private publication/public-key repair. Only an absent/unusable grant pin permits a transient
  authenticated existing WrapMark unwrap. Its zeroizing private Secret is dropped before
  any further await; a final fresh pin read supplies the usable CAS or exact mark refusal.
  Usable-pin grants and revokes avoid this mark path.
- maxShares uses the current recorded ledger including gone entries before conversion/sweep.
  At capacity the owner can revoke or perform an unmatched revoke that sweeps gone entries,
  then grant again. Revoke-by-address selects the distinct-id union of all matching recorded
  addresses and the resolved id only if it is a current grant; multiple ids are ambiguous.
- Authenticated converted=true rings skip conversion and the historical event payload scan
  for later grants, no-ops, level changes and rekeys. Complete valid scanning remains required
  before an unconverted ring first publishes converted=true and a grant.
- New sharing traces contain fixed details, ids, levels and counts. Existing authentication
  AccountName metadata is preserved only in its exact inherited success/failed-auth fields;
  all-event canary checks retain this boundary rather than globally filtering auth events.
- A test_mode-only staged-calendar helper and narrowly scoped test bridges can construct
  disposable fixture state. Production domain constructors and external authority boundaries
  remain restricted. Two common HTTP pause forwarders are no-ops without common test_mode.
- Pure delegated builders use owner-only snapshots before first-run finalize. A overrides
  only directly-owned listed aliases' routing force_send/rsvp fields; R controls retention.
  They preserve raw None/NothingToSend/Some(empty) distinctions and reuse upstream inner
  builders. Writer info remains unmerged and includes groups. A shared owner/writer group
  is owner-local for the organizer gate and writer-own for SENT-BY placement.

- Opaque tagged fan-out messages expose a read-only recipient_count for combined recipient-limit enforcement; payload/provenance fields and Self construction remain restricted.

- Delegation resolution and pure builders share the owner-local organizer gate. Direct alias
  preparation propagates outer errors; per-recipient Failed results remain request-local and
  are reused once with attempted5.1 and no duplicate lookup trace. The combinator alone
  removes R from upstream attempts and produces retained Self provenance.
- Fan-out keeps merged sender, unmerged writer and owner-only account roles distinct. Owner
  addresses define token locality, while writer keys, permissions, pins and changed_by remain
  the writer's. Self-inbox permission is checked per tagged message/address attempt; known
  owner identities on the gated path are excluded before key/permission I/O. A raw Failed
  outcome retains5.1 because the account identity is unknown.
- All delegated writes exclude the owner's source event from writer withheld-state fallback,
  refusal read/create/refresh/consume, full-state emission and retirement, including coincident
  numeric document ids. First-use key and pin decisions remain active.
- A receiver candidate preserves its named sender and actual SENT-BY absence separately from
  invalid presence. Delegated assertions require a real mailto Location URI and quiet lookup
  of different named/writer key accounts matching changed_by. Named identity feeds merge and
  token matching; owner event/grant/lease state is not consulted. Existing incoming scheduling
  validation and retained-history behavior remain authoritative.
- Owner shared-calendar DELETE invalidates fresh validated ACL grantee caches after successful
  commit. Rejected writes do not evict, and unrelated sessions remain cached.

## Task 10 DAV admission, discovery, properties and sync

- Calendar admission preserves fresh ordinary-account classification/error semantics and
  owner-key admission before foreign-session restrictions (T10-G). Foreign sessions require
  their own keys, neither impersonation form, and exact Calendar collection access; fresh
  per-operation resolution still requires every requested own-id ACL bit. Scheduling and
  MKCOL retain their owner-key gate.
- Foreign home discovery/listing/propname use fresh eligibility. Only Forbidden and precise
  missing calendars are omitted; corrupt eligible records remain errors. Direct calendar
  requests resolve the addressed parent only; home/assisted views evaluate their eligible set.
  A valid addressed foreign home may return207 with no eligible child calendars.
- Incremental sync fetches vanished paths before fresh eligible archives, derives each current
  prefix and boundary from the same archive, and preserves eligibility independently of log
  intersections. Pending boundaries refuse all incremental tokens; initial sync remains valid.
  Tokens at/below max grant/rename boundary refuse before filtering/pagination. Filter requested
  subtree and current eligible prefixes before counting/emitting tombstones.
- Calendar PROPPATCH opens/edits the authenticated writer's own personal bundle plus shared
  dead properties and preserves every other unopened entry/ring verbatim. Calendar conflicts
  remain immediate409. Event writes retain their original event CAS and existing bounded retry.
- HTTP differential fixtures preserve incoming rejection rules, including subaddress-only and
  shared-group organizer history. No copy normalization removes SEQUENCE/status/attendee params.


The following is a lossless snapshot of every progress entry containing `Ruling`, in source
order, including context/index entries. Each substantive entry preserves its decision,
reason, explicit cost (where recorded), and restrictions. No cost has been invented for
an entry that recorded none. Historical R11-19b and partial R11-48 statements are superseded
by Appendix B/current Task11 dispositions above; they are retained to expose the decisions
made during execution. Routine task-status records are historical, not fresh acceptance.

### Progress entry 32

## Rulings

### Progress entry 33

- Ruling: Reuse the already isolated linked worktree on a new implementation branch — preserves reviewed documentation branch and unrelated file — cost if wrong is reversible local branch management.

### Progress entry 34

- Ruling: Primary stages and commits after independent review; agents do not commit — repository ownership instruction supersedes skill template's implementer-commit wording — no change to per-task review gate.

### Progress entry 64

| C1 | P:584–596 gives `za_complete_boundary(own_commit: Option<_>)` and permits returning the pending archive on a lost **repair** assertion. S:1435–1437 requires a lost **step 2** to reread and retry with a post-archive-read change id; S:1072–1077 also describes completion after the grant commit. Treating the helper's pending-return allowance as applying to every invocation would omit that step-2 retry. | Accepted by primary during this scan: scope the single-loss/pending return to opportunistic owner DAV read repair. A grant/rename's own-commit completion that loses discards its old commit id, rereads the archive, reads `last_change_id` after it, and retries once as the second-kind completion. A further loss leaves pending and must be reported accurately; post-commit invalidation still runs. Never clear newer flags with the earlier commit id. Carry this distinction into the Task 5/7 brief. | Ruling accepted for Tasks 5 and 7; no Task 1 blocker. |

### Progress entry 196

- Ruling C1: own-commit grant/rename completion losing its first assertion re-reads the archive, then last_change_id and retries once; an additional loss leaves flags pending. Opportunistic owner DAV read repair may return pending after one loss. This follows binding spec §5.2; cost if wrong is conservative extra full sync. Carry into Tasks 5 and 7.

### Progress entry 199

- Ruling T1-G: add OwnerRingHeader::next_generation() -> Result<u32, SealError> using checked_add — Task 1 explicitly tests generation exhaustion, Task 7 will consume it — u32::MAX remains valid stored generation but cannot rotate. No format change. Cost if wrong is a small removable utility.

### Progress entry 214

- Ruling T2-MOVE: envelope-only cross-calendar rewrap validates the source stored envelope/sole parent first, rewrites only its key envelope, then Task 5 changes event names/parent before commit. It cannot require unchanged names to match source and destination simultaneously. Normal read/unseal retains exact sole-parent matching; plaintext full seal may receive destination names first. Preserves envelope-only contract, no read-time relaxation. Carry to Task 5.

### Progress entry 216

- Ruling T2-P: ZaKeyDomain privately binds request-local principal identity (owner for owner domains, resolver-verified grantee id for foreign domains). Public calendar view/property-write personal_id must match. No stored/wire change. Internal owner-authorized grant/conversion bundle preparation remains separate and can seal intended grantee snapshot entries. Task 3 must bind domain principal to authenticated token id; Task 7 prepares entries through explicit owner-only path.

### Progress entry 218

- Ruling T3-F: public codecs cannot bootstrap staged shared fixture bundles while production domain construction stays restricted. Task 3 may add a test_mode-only groupware test-utility module (e.g. sharing/test.rs, registered under cfg) taking owner SessionKeys plus a prepared ring and plain Calendar/entries, using private domain + bundle helpers + Task 1 assembly, and returning a sealed Calendar. It performs no store writes; tests own staging/cleanup. Production constructors/helpers remain crate-private. This is necessary test-fixture plumbing, not a production authority API or pause/fault hook. Cost if wrong is removable test-only utility; carry to Task 5 reuse.

### Progress entry 241

- Ruling T3-I: reject a token whose attached SessionKeys account differs from its own account, and reject za_is_impersonated before CK use. Owner with matching own keys and Impersonate permission still uses the owner arm. No widening of za_keys_for.

### Progress entry 242

- Ruling T3-N: represent a freshly missing calendar as Store(StoreEvent::NotFound), preserving existing adapter 404/omission for precisely NotFound. Other store failures stay errors; no new ZaCalendarError variant.

### Progress entry 244

- Ruling T3-C: preserve CollectionKeyId identity and add session-only successful-authentication witness metadata for exact fresh grant-ring bytes. Use a whole-ring digest witness bound atomically to the cached Secrets (integrated state, no uncoordinated proof/key maps). Fresh archive, own-id ACL/all required bits, impersonation, header/domain/listed generation and complete structural checks precede cache use. A hit requires witness equality for the complete requested key set; publish keys+witness only after successful whole-ring opening. Ordinary single-key replacement invalidates its witness. A changed ring misses and must authenticate, including a warm tampered wrap. Owner always opens its fresh ring for encrypted converted/reverting/ledger state. Witness is unwrap-success metadata, never permission/current-generation authority. Narrow Task 3 scope addition: vault/session.rs plus adjacent units; no stored layouts/dependencies. Independent bounded review task-3-cache-contract-review.md supports compatibility with spec 3.6 and preserves its rejected-alternative distinction (not hash-keying the CK identity). Require warm-corruption and unchanged-ring avoided-unwrap coverage plus atomic/replacement negatives.

### Progress entry 255

- Ruling T3-B: owner context boundary.pending aggregates the owner header and any structurally validated grant-header pending flag. A new grant can be pending while owner header is clear; Task5 explicit owner read repair must notice that state. No grant decryption, new write, or repair inTask3. Keep per-principal boundary ids as defined. Cover pending-grant-only positive and clear-state negative. Carry toTask5. The already-started za run is pre-correction evidence; distinguish its source revision from final checks.

### Progress entry 273

- Ruling T4-T: CalendarEvent::update resets modified and insert resets created/modified (storage.rs188,215–217), so retries cannot call these blindly at a later time. Narrow timestamp-preserving writer variants in the already-owned storage.rs are approved. Existing entrypoints delegate and retain ordinary behavior; capture one initial assembly timestamp for both attempts, reuse indexing/commit-point logic, and return ETag for the actual committed retry envelope. Cover preserved metadata/index/alarm values. No general batch-operation cloning or unrelated public API expansion.

### Progress entry 300

- Ruling T4-L (independent bounded contract review): retain abandon for confirmed fanout loss and first commit loss with unchanged calendars. A second create commit loss is opaque; await the existing fenced lease.release before the same bounded UID-retake/outcome path. Use DAV-local disposition enum/flag or equivalent DRY helper, no lease.rs API change. Release cannot clear a successor; false is inconclusive (successor/store failure), never infer lost ownership from it. Keep confirmed-loss behavior/traces,412/503 outcomes, no third calendar read, no archive retry or fanout replay. Cover still-owned second loss with immediate reacquisition, successor preservation and unchanged known-loss controls. Existing initial COMMIT_PAUSE site stays; retry can reuse that one-shot slot for deterministic second-loss testing if necessary. Report task-4-create-loss-contract.md.

### Progress entry 302

- Ruling T5-R: resolution remains read-only. Explicit owner-pending boundary repair may be wired through shared DAV mutation wrappers; narrow adapter-only additions to calendar/mod.rs and delete.rs are approved, with update/proppatch only if necessary. If owned drain/contacts resolutions require the same explicit wrapper, narrow adapter-only edits there are approved too; no scheduling/retry changes. Use the repair-returned fresh archive/context for subsequent fences, retaining pending on lost opportunistic repair. No implicit write in the groupware resolver. Task5 preparation must enumerate actual covered calls; Task7 ring mutations complete boundaries under their own contract. Case24 COPY half is Task5; actual first-grant race activation carries to Tasks6/7.

### Progress entry 313

- Ruling T6-C: BatchBuilder may split at event commit points (>1000ops or >5MB), and commit_batch loses partial-prefix counts on an error. Task6 may use individually fenced event writes within logical groups of at most100, updating counts only after each acknowledged commit; pauses/interruption cuts remain at the named phase/group boundaries. Every independent event write gets current calendar/vault/Account/event assertions. This satisfies the at-most100 contract without global batch API changes or unknown committed-prefix accounting. A multi-event alternative must explicitly prove physical fencing and committed-prefix counts before use. Final scans/walks and HTTP budgets remain unchanged; record actual granularity in benchmark/outcome. No Task6 source work until READY after Task5.

### Progress entry 324

- Ruling T5-C: key-account collectionCOPY may split indexed calendar create/overwrite from child links. Commit create/overwrite with source/old-destination fences, read actual new destination archive/context back, then fence it plus source/all-existing parents and original retained child archives in every physical child chunk. Shared readback =>403 before links. No preparedhash or storeAPI changes; ordinarypath unchanged. Never replay creation/newid on child failure; bounded calendar retry applies child intent with original childCAS/timestamps. A previously committed prefix may fail originalCAS409 rather than duplicate links; inherited partial-failure behavior is explicit. Per-child indexed commit loop is allowed if it is the simplest safe construction. Applicable old-destination fences remain with overwrite chunks until deletion; schedulingoff and originalcutoff. Require deterministic new-destination/chunk conversion race regression via existing test-only pause slots.

### Progress entry 328

- Ruling T7-M (independent report task-7-pin-mark-contract.md): literal plan step3 "never open" conflicts with required absent/unusable-pin authenticated WrapMark classification; current mark exists only in AEAD purpose and existing APIs decrypt private-key plaintext. Binding spec4.1 says the key is not needed for sharing; spec4.3 requires the mark. Refine the plan explicitly: grant/revoke never call za_private_key/za_self_check, publish lazy session private-key outcome, repair/heal owner public key, or use that private key for sharing. A grant with absent/unusable pins may transiently unwrap the existing private-key wrap solely for its authenticated mark, immediately dropping the zeroizing Secret before any further await, response, cache, guard or conversion. Revoke does not read pins/mark or unwrap. Usable pins take the no-mark fast path; otherwise initial unusable pin read -> fresh mark read/drop -> final fresh pin reread; use final usable read/CAS or exact pinned/legacy/neither refusal table. This preserves concurrent first-pin ordering without claiming snapshot consistency. Existing PRIVATE_KEY_FAIL_ONCE proves self-check admission only, never zero unwraps. Cover raw pin/vault nonmutation, exact errors, no lazy outcome/public repair, usable-pin grant and revoke with corrupt private wrap, concurrent first-pin ordering and pin-CAS loss. No custom tag verification, new primitive/field/dependency/layout/permission. Task7 is authorized to clarify that one plan sentence to this accepted interpretation and record the explicit exception; do not alter binding spec or claim the original blanket sentence was literally met. No Task7 source work until READY after Task6.

### Progress entry 335

- Ruling T5-D: effective-write-mode equality is insufficient for MOVE. A reverting shared source can still contain CK envelopes; moving to unshared must open sourceCK/rewrap toEWK even when both domains currently writeEWK. Compare actual cryptographic domain identity, including shared calendar/domainid; distinct unshared calendars retain the same ownerEWK domain and accepted storedbytes. Reverting destination encoder selectsEWK; same-calendar rename retainsbytes. Implementer found gap in self-review before freeze; add sourceCK/reverting->unshared and reverting-destination regressions, actualRED beforecorrection. Current obsolete run may be interrupted only after completing then-currentTask5cases and recordno-pass, never sourceeditwithCargoactive.

### Progress entry 356

- Ruling T5-I1: retain fresh raw archive fences on overwritten-event off-path parents, without deriving keys or requiring authority there. Addressed source/destination permissions and source-linked refusal stay. This refines earlier all-parent-context ruling to preserve explicit unlink recovery. A narrowly test_mode-only actual-path bridge may support grantee regression while HTTP remains closed untilTask10; no production authority API expansion.

### Progress entry 370

- Ruling T5-V: no public/cross-crate shipping authority API, constructor or HTTP admission may widen. Narrow pub(super) parent access to existingprivateMOVE helper for cfg(test_mode) bridge is approved internal plumbing; lexical shipping visibility is recorded in scopedreview, no concreteauthoritycounterexample found. Earlier blanket brief sentence refined explicitly. Carry note into finaltriage.

### Progress entry 377

- Ruling T6-N: rewrapped counts each acknowledged DEK-envelope rewrap write in the current invocation, forward or CK->EWK rollback; sealed counts acknowledged full LegacyPlaintext seals. Ring-only writes, skips, failed/injected attempts and previous invocation work do not count. A rollback then reconversion may count the same event twice, reflecting two actual committed rewrites. Counts-only handoff/HTTP accumulation cannot deduplicate distinct ids across resumes; this follows spec4.3 actual-DEK-work wording. Explicit store-backed resumed-rollback/forward count assertion required; Task7 adds each returned committed delta exactly once. Cost if wrong is count semantics refinement, no persistence/crypto/interface change.

### Progress entry 381

- Ruling T6-P: binding spec1.2/3.3 explicitly clears entry.color and forbids personal bundle color. CalendarPreferences.color is upstream JMAP-only, never DAV-written; Apple colour/order are actual calendar dead_properties. Preserve name/description/customTZ/sort/flags/defaultalerts and actualdeadproperties. ExistingTask2codec normalization of admitted legacy personal/JMAP-only Some(color) stands as spec-driven exception to broad field-preservation wording; do not invent Apple property migration, overwriteexistingdeadprops or widenformats. Cover actualApplecolor/order preservation throughconversionANDrollback and disclose rarelegacyfieldnormalization inreport/outcome. Costifwrong is discarding an old JMAP-only preference; preserving it instead would require an explicit format/migration decision.

### Progress entry 385

- Ruling T6-U: original event CAS remains binding. A held UPDATE whose event was actually rewritten by conversion has both calendar and event version changes; after one calendar rebuild its original event CAS must return409, even for envelope-only conversion. Case21 requires bothcreate/updatevariants and final03, not unconditional204 despite concurrenteventrewrite. Preserve completed-conversion conflict variant with sourcepayload/metadata notchangedbyrefusedupdate and validCK. Add cal-only success variant: holdUPDATE, pauseconverterafterphase1/beforewalk, releaseUPDATE=>204/newCK usingoriginalstillmatchingeventCAS, thenreleaseconverter=>authenticatedCKskip/readiness. HeldCREATE201 retained. No DAV/Task4 production change or neweventCAS adoption. Costifwrong is clientoptimistic retry; relaxingCASwouldrequireseparateapproveddesign.

### Progress entry 389

- Ruling T6-M: full parentage walk retains event IDs only, each logicalgroup loads atmost100 archived payload snapshots, and finalvalidation streams currentarchives. Rewrap pause remainsafterphase1/beforewalk; between-group pause mayfollow nextgroupreads beforeitswrites, preserving deterministic actualevent-CAS losses while boundingpayloadretention. Everyphysicalwrite retainsguards/calendar/eventassertions and committedcounts; no storeAPI change. Report O(N IDs)+<=100payloads andactualper-eventcommitgranularity inbenchmark/outcome.

### Progress entry 409

- Ruling T7-H: two new HTTP-only deterministic pauses (pin mark before final pin reread, opaque-loss before fresh Account then vault classification) are permitted to prove required ordering. Direct HTTP-test-mode calls into common::za::test_hooks fail the standalone prescribed HTTP unit configuration: http/test_mode=[] does not enable common/test_mode. Authorize exactly two existing-pattern always-available forwarders in common/za/mod.rs, no-op without common test_mode, and route the new HTTP seam calls through them. Add that file narrowly to Task7 scope; no Cargo/features/dependency/layout/settings change. SHARE_COMMIT remains called directly in mutations.rs and needs no forwarder as planned. Cost if wrong is two internal wrapper functions/test overhead; omitting the boundary breaks required standalone HTTP acceptance. Verify cargo test -p http@0.16.26 --features test_mode before final freeze. No edits during active Cargo.

### Progress entry 416

- Ruling T7-G: maxShares counts the calendar's current recorded ledger, including gone entries, before conversion/apply, following spec4.3's explicit current-count refusal. The spec7.5 sweep occurs in a successful mutation; a refused new grant writes nothing. At the cap with a gone grant, return `too many shares`; an id revoke of that grant, or an unmatched-target revoke that still sweeps gone entries, reports changed:true and frees capacity, then a new grant succeeds. Cost if wrong: an extra cleanup request at the cap; owner can perform it without granting new access or broadening the bound. Require the full-gone-ledger refusal/revoke-sweep/retry fixture and document the condition in API/outcome. No binding spec change.

### Progress entry 420

- Ruling T7-A: stable grant ids do not imply unique recorded addresses. Alias A can move from grantee1 to newly granted grantee2, so both legitimate ledger records hold A; moving A back makes `.find()` wrongly hide the second recorded match. Revoke-by-address takes the distinct-id union of ALL exact normalized ledger matches and the resolved account id only when it names a current grant. Zero => requested-target no-op subject to gone sweep; one => select that grant; multiple => existing `address is ambiguous, revoke by grant`, no mutation. Id-based revoke remains exact. Add/observe actual alias-cycle RED before correction, preserve duplicate-address grant validation and do not impose new uniqueness or change binding spec. Cost is requiring id revoke in genuinely ambiguous duplicate-address states, as intended by stable-identity design.

### Progress entry 426

- Ruling T7-W / primary integration finding: HTTP success currently uses only ZaShareApplied.changed. Empty-calendar same-target race: request A acknowledges phase1 then parks at SHARE_REWRAP_PAUSE; B completes conversion/grant/boundary; A resumes converted and apply no-ops, yet A wrote phase1. Returning changed:false with zero event counts contradicts spec4.3's false-only-when-nothing-written rule. Track actual acknowledged conversion writes, including ring-only writes, across request resumes. Preserve the two-field ZaConversionCounts and wire count schema. Authorize a narrow tracked-converter entry point with request-local mutable write witness plus the existing API wrapper if required; set only on acknowledged commits, never inferred from precheck/observed versions. No stored layout or authority/key-constructor change. Require actual deterministic RED for after-phase1 writer => changed:true, and before-phase1 loser with no acknowledged writes => changed:false. This covers both exact attribution and metadata-only conversion work. Cost: small internal signature plumbing, not global store API redesign.

### Progress entry 470

- Ruling T9-R: public ZaFanOutMessage::recipient_count()->usize is approved read-only metadata for DAV combined/filtered recipient-limit enforcement while message/provenance fields staycrateprivate andSelfconstructorsolecombinatorproducer. It adds no authority/mutation/format; costifwrong is a small removableaccessor. ExactTask9files unchanged; schedulingstaysbeforepreservedcommitretryloops.

### Progress entry 524

- Ruling T10-G: Calendar gate preserves the existing fresh ordinary-account registry classification/error semantics before key-account admission. Owner session keys admit the owner before the foreign-session/Impersonate/ACL branch, including an administrator reading its own calendars. No lookup-order rewrite or authority expansion. Cost: an owner with keys still observes an existing registry lookup failure; this interpretation remains subject to independent source review.

### Progress entry 559

- Ruling M6: mutation row6 means remove all physical endpoint-calendar ARCHIVE assertions, not only redundant context.assert_in. Primary verified same fence also emitted by Calendar.update -> ObjectIndexBuilder update (common/storage/index.rs351). A temporary BatchBuilder-internal exact owner/Calendar/id/ARCHIVE assertion filter called only from za_share_apply after batch assembly is allowed if required, preserving Account/vault/pin/otherdocument guards and all writes/index/log operations; full helper+call diff/counter considerations/case20 result/restoration must be recorded. No permanent helper/API addition or global calendar/assertion bypass. Cost if wrong: invalidate/repeat row6 qualification; no shipped behavior change.

### Progress entry 565

- Ruling M2: the absent AccessToken invalidation mutation may target revoke only, keeping prerequisite grant publication functional so the required real case17 post-revoke residency assertion executes. Actualpatch keeps grant invalidation and removes revoke duty, not resolver/authentication/cleanup behavior. Record branch scope transparently; cost if wrong is repeating row2 qualification, no shipped behavior change. Primary inspected exact recorded patch and source restoration.

### Progress entry 567

- Ruling M9: to reach designated case3 without earlier admission setup masking it, prefer a temporary fixture-scoped omission of sealed-event conversion rewraps on that existing case3 calendar (public slug/owner identity), leaving guards/finalvalidation/counters/allassertions and other fixtures unchanged. No permanent hook/API or crypto alteration. Exactpatch must be reviewed beforeexecution and causal required200/NotSealed plus storedEWK evidence preserved. This tests the same physical omission in its prescribed case; cost if wrong is repeating row9 qualification, no shipped change.

### Progress entry 579

- Rulings R11-13b: row 13 attempt 3 may scope the condition to case 7's own grant by any key at the seam that makes it the first affected grant (calendar name plus generation count or grantee id), after the worker maps every earlier new grant on that calendar; if no key isolates it, record row 13 as detected by the production correspondence guard at the first multi-generation grant, with the by-construction argument that the guard refuses every new grant whose generations differ from the owner's (so case 7's grant cannot succeed), for the original chat to adjudicate. R11-15: oracle wave adds a case 11 race: park an edit grantee's PUT update after its snapshot (existing PUT pause), downgrade the grantee to view through shares/grant, release, require 403 and a byte-identical archive; then re-run row 15. R11-19: the oracle wave either demonstrates observational equivalence of the assisted-discovery eligibility omission (every appended path is re-resolved through the resolver's own eligibility check before emission) or designs an existing-seam oracle that tells them apart. Oracle drafting runs in parallel with batch 3 without Cargo and without tracked-file edits; drafts land in task-11-evidence/oracle-wave-1/.

### Progress entry 582

- Ruling R11-22: rows 22, 23 and 24 each get one attempt scoped by event UID to case 10's fixtures (sharing_http_schedule_contracts), leaving case 25's UIDs original, so case 10 executes under the condition (the M9 and R11-13b precedent). Case 25's detection in attempt 1 stays recorded as additional evidence. Cost: three runs; if the original chat prefers case 25 as the detecting case, attempt 1 already holds that evidence.

### Progress entry 585

- Ruling R11-19b: row 19 is not observationally equivalent. With a corrupt eligible calendar the original fails Bob's assisted own-home listing with 500 (accepted contract: "only Forbidden and precise missing calendars are omitted; corrupt eligible records remain errors"), while the mutant appends the path through upstream shared_containers and answers 207 with one 500 item. Other differences (planted non-level ACL bits, an undeserializable archive, a mid-request deletion as a 404 item, the Unauthorized events and calendar_reads entries for ineligible calendars) are recorded, not used as oracles. Oracle 19 enforces the accepted contract on Bob's own home, which the fixture checked only for Alice's home. Cost if wrong: the original chat may prefer the per-item 500, which would make oracle 19 wrong and row 19 an open question.

### Progress entry 589

- Rulings: R11-26: one attempt scoped to case 12's event UIDs (`move-` prefix), R11-22 precedent. R11-29: oracle wave 2 adds, without reordering existing steps, an unlimited sync requiring no /private/ href and a limit-1 sync whose token carries no continuation; reorder only if the drafter shows these cannot distinguish. R11-31: a hook-arrival Elapsed is a timeout, not a kill; oracle wave 2 adds an unpaused rename into a former slug before the paused rename (the kept old-slug token must answer 403; a full sync shows no private tombstone). R11-33: one attempt scoped to the paused-rename fixture's destination calendar (`reused`); attempt 1's detection in case 1's pending-grant sub-case stays recorded. R11-25 and R11-32 extend the condition to every physical copy of the same rule on the path (M6 style); flagged for the original chat's review.

### Progress entry 599

- Rulings: R11-37: scope to exact UID case10-sole-decline. R11-38: scope to UID prefix case10-. R11-43: scope to exact UID case10-sole-delegated. All three follow R11-22; the predicted earlier case 25 detection is not executed and not claimed. R11-46: (a) the direct assertion on the public gate (delegated_attendee_and_alias_edges, za_resolve_delegation(...).is_none()) is an API-level oracle on the exact rule and counts as detection, recorded as such; (b) because the ledger asks for an observability analysis, the first mutant attempt carries the audit's traces and must report the held event's schedule tag around Bob's attendee-only ACCEPTED and DELEGATED writes in case9-attendee-reply (which runs before the gate assertion); if the tag differs, the coordinator approves a test-only oracle requiring the stored schedule_tag unchanged across those writes (RFC 6638: attendee-only changes keep the Schedule-Tag), then a green run and a re-run; if nothing differs, the traces and the code paths are recorded as the HTTP-level equivalence analysis. R11-48: killed at case 25 force-sole is the row's kill (first of its two required cases, row 7 precedent); case 10's half is insensitive by construction (case10-sole-delegated has no SCHEDULE-FORCE-SEND and no DTSTART edit); no case10 attempt; flagged for the original chat as a fixture gap against spec 11.6's wording.

### Progress entry 604

- Ruling R11-46(b): not equivalent. The mutant's traces in case9-attendee-reply show the held external invitation's stored schedule_tag rising by one on each of Bob's attendee-only writes as Alice (ACCEPTED: Some(2); DELEGATED: Some(3); before: Some(1), derived from update=true), with identical built/retained counts, status lists and inbox counts. The original keeps the tag (gate refuses, no second run; the first run's REPLY is not from the organizer). A changed Schedule-Tag is client-visible (RFC 6638 3.2.10: an attendee's own changes to its participation do not change the Schedule-Tag of its scheduling object resource), so the HTTP difference is observable. Approved test-only oracle 46 (wave 3): the held event's Schedule-Tag (as served to the owner on GET, or the stored schedule_tag where the fixture reads stored state) must be unchanged across Bob's ACCEPTED and DELEGATED writes. Drafted now (Opus, high, no Cargo), applied after rows 47-57 release Cargo, then a green run of the unmutated suite and a re-run of row 46 as a new attempt that must fail at oracle 46. Cost if wrong: if the original also changes the tag, the green run fails and oracle 46 is withdrawn, row 46 then resting on (a) and recorded as equivalence-unproven.

### Progress entry 622

- Ruling R11-19-review: binding globalcontract andspec10 permit a request OR item500. Reject externalR11-19b strongerwhole500interpretation; its row19attempt3 proves packaging difference only and must not count as a valid required-case kill. Retain historicalreceiptwith explicitreviewcorrection. Correctoracle toallowbothvaliderrorforms. To qualify the prescribed omittedeligibilityrule, a temporarylogicalfault may removeduplicatefresh-eligibilitycheckson the assistedown-home/foreignpropname path (preappend plusperitem), underM6, leavingdirectpaths/crypto/unseal safeguardsunchanged. Preferexistinggroup/own-id fixture toshowforbiddenhrefemission; inspectexactpatchbeforeexecution. If noforbiddenbehaviorisdemonstated, recordhonestequivalence/unresolvedcoverageinsteadofforcingkill. Costifwrong: revisitrow19qualification/errorgranularity; no shippedproductionchange.

### Progress entry 624

- Ruling R11-validation-review: accept R11-25/R11-32 as logicalfaults removingeveryphysicalcopyofthesamepathrule underM6, withdocumentedlimits (notproofthatoneisolatedhint/fenceomissionisbypassed). AcceptR11-13asM9positivegrantpreemption, notreader-sidecryptographicproof; acceptR11-22familyoffixture/callpathscopedruns withoriginalpreemptingattemptspreserved. Costifwrong: repeataffectedqualification, no shippedchange. Row48remainsincompleteuntilcase10sensitivetestandreceipt.


## Appendix B: external Task 11 rulings and current dispositions

These are the committed mutation note's current adjudications. They supersede conflicting
historical qualifications in Appendix A and retain each condition's scope and cost/status.

### Task 11 rulings

Made by the Task 11 coordinator during execution, for the original chat's review. Rulings M2, M6 and M9 come from the plan's earlier integration rulings: M2 scopes row 2 to revoke; M6 removes every physical copy of the row's rule on the exercised path; M9 accepts as a kill a causally established failure of the required case's positive acceptance path, with its exact origin and the stored state recorded.

- R11-7a: row 7 is killed at case 21; attempt 2 narrows the same omission to grantee writers (writer account differs from the calendar's owner) so case 22 executes under it; supplementary evidence only.
- R11-7b: case 21's held-create variant checked content only through a reader that accepts 0x01 rows, so it lacked spec 11.2's "every event ends 0x03"; oracle 07b adds the structural envelope check, validated by a creates-only narrowing of row 7 (attempt 3).
- R11-10: row 10 is judged by the M9 standard without scoping, since its global skip reached case 3 first; its added traces are observation-only.
- R11-11: row 11's executed patch is revoke-only (the draft also removed case 8's re-key rotation). Attempt 1 survived; oracle 11 adds case 6's generation assertions.
- R11-13 and R11-13b: the production correspondence guard (`za_replace_calendar_entries`, `crates/groupware/src/calendar/sharing/format.rs:637-645`) refuses every new grant whose ring generations differ from the owner ring's, so under row 13 the first multi-generation grant fails before case 7's reader oracle. Attempt 3 scopes the condition to case 7's own grant (`http-sharing` with at least seven generations; the trace shows the seven earlier new grants unscoped and passing) and is killed by the M9 standard: 500 with traced `Structure("grant header correspondence")`, nothing published.
- R11-15: DAV writers check the container ACL from the request snapshot before the resolver, so the resolver's bit check is only observable in a race; oracle 15 adds that race. Attempt 1 survived.
- R11-19/R11-19b (historical, superseded): the external coordinator classified the assisted own-home207 with a500 item as a kill because the original returned request500. R11-19-review rejects this interpretation: global contract89–92 and spec10 allow request OR item500. Attempt19/3 is invalid qualification, its raw receipt is preserved, and its packaging assertion is replaced with the accepted error contract. A fresh case11 eligibility fixture/mutation or demonstrated equivalence is required.
- R11-19-review: M6 permits removing both physical fresh-eligibility checks only for assisted foreign entries on the caller's own home (preappend plus peritem propname). Direct foreign requests and crypto/unseal guards remain intact. The group-member fixture distinguishes a forbidden href from correct omission; no stricter error-packaging contract may be invented. Attempt4 now fails at that exact forbidden href, not the invalid packaging oracle.
- R11-22 (applied as R11-26, R11-37, R11-38 and R11-43): when an earlier case (usually case 25) detects a condition first, one attempt applies the same physical change only to the required case's own fixtures, keyed by event UID, leaving every other path original; the earlier detection stays recorded. Rows 22 to 24 and 38 use the `case10-` prefix, 37 the exact UID `case10-sole-decline`, 43 the exact UID `case10-sole-delegated`, 26 the `move-` prefix. R11-33 scopes row 33 the same way to the paused-rename fixture's destination calendar `reused`. Rows 4 and 5 (endpoint-only guard copies) and 21 (Upstream-provenance messages only) are scoped by call path under the same principle. Cost: if the original chat prefers the earlier case as the detecting case, the first attempts already hold that evidence.
- R11-25 and R11-32: the condition removes every physical copy of the same rule on the exercised path (M6 style). Row 25 also removes the target sole-parent check from a temporary copy of the rewrap step; row 32 routes the rename through a temporary copy of `za_complete_boundary` that takes the hint as given, because the draft alone is caught by the completion's own identity check.
- R11-29: oracle 29 adds, without reordering existing steps, an unlimited sync and a limit-1 sync check. Attempt 1 survived (the limit-1 truncation emits the granted tombstone first).
- R11-31: a hook-arrival `Elapsed` is a timeout, not a kill (attempt 1); oracle 31 adds an unpaused rename into a former slug before the paused rename.
- R11-46: (a) the direct assertion on the public gate (`delegated_attendee_and_alias_edges`, `za_resolve_delegation(...).is_none()`) is an API-level oracle on the exact rule and counts as detection (attempt 1). (b) The attendee side is not observationally equivalent over HTTP: the mutant raises the held event's stored and served Schedule-Tag by one on each of the writer's attendee-only writes, everything else equal; oracle 46 requires it unchanged. Attempt 3, the default mutant with attempt 1's exact patch lines, fails at oracle 46 (`sharing.rs:9233`): after Bob's attendee-only ACCEPTED write as Alice the served Schedule-Tag is `"2"`, not the owner's `"1"`; the traces show the attendee-side second run built one message, retained none, and the stored schedule tag rose to 2. The row's first two attempts stay recorded.
- R11-48 (historical partial qualification): attempt1 validly killed case25 force-sole. At that fixture version case10 had no ForceSend/DTSTART effect and no attempt; the row was incomplete. Fix round1 adds ForceSend to the delegated sole create plus a hard stored-source parameter-absence assertion, and attempt2 now detects the missing finalize in case10 under transparent exact-UID scoping. Both required halves are qualified.

### Review disposition and remaining acceptance

- R11-19-review rejects historical R11-19b and invalidates attempt19/3. Attempt19/4 qualifies the fresh-eligibility rule under M6 through an actual forbidden href. Request500 or an exact corrupt-calendar item500 is accepted.
- Row48 now has both required halves: original global attempt1 at case25 and exact case10-scoped attempt2. The earlier insensitive fixture and missing attempt remain documented in the historical record.
- R11-validation-review accepts R11-25/R11-32 as logical faults removing every physical copy on the path, R11-13 as an M9 positive-grant preemption, and R11-22 scoped runs with their original preemptions preserved. These are qualified observations with the recorded limits, not broader claims.
- Duplicate `ck_envelope` logic is extracted to one module-private helper; existing assertions stay at their call sites. Scoped independent rereview approved all3 corrections with0 new findings (`task-11-fix-round1-rereview.md`); restored final gates passed as recorded below.
- Deferred Minor findings remain outside this fix wave: row35 changed-body coverage and inherited compiler/linker warnings. No native-client or Task12 acceptance is claimed.


## Appendix C: exact temporary benchmark instrumentation

Decode the JSON object below and concatenate `lines` in order as UTF-8. The result is the
complete applied/formatted unified patch, including its newline/whitespace bytes; its SHA256
must equal `sha256`. Source was restored exactly after this patch's run. JSON escaping avoids
Markdown trailing-whitespace edits changing patch identity.

```json
{
  "encoding": "UTF-8",
  "sha256": "050ec1f1bf53f2e9278288743ab601cb0552f16cb57b294792609d008be7d42b",
  "lines": [
    "--- a/tests/src/za/sharing_races.rs\n",
    "+++ b/tests/src/za/sharing_races.rs\n",
    "@@ -2438,7 +2438,17 @@\n",
    "     let request = |slug: &str, address: &str| json!({\"username\":USER,\"password\":STRONG,\"calendar\":slug,\"address\":address,\"level\":\"view\"});\n",
    "     let mut slugs = Vec::new();\n",
    "     for slug in [\n",
    "-        \"race\", \"init\", \"rewrap\", \"revert\", \"rollback\", \"restore\", \"cut\", \"large\", \"other\",\n",
    "+        \"race\",\n",
    "+        \"init\",\n",
    "+        \"rewrap\",\n",
    "+        \"revert\",\n",
    "+        \"rollback\",\n",
    "+        \"restore\",\n",
    "+        \"cut\",\n",
    "+        \"large\",\n",
    "+        \"other\",\n",
    "+        \"benchmark-full\",\n",
    "+        \"benchmark-resume\",\n",
    "     ] {\n",
    "         client\n",
    "             .request(\"MKCALENDAR\", &format!(\"{HOME}{slug}/\"), MKCAL)\n",
    "@@ -2871,8 +2881,9 @@\n",
    "         .expect(200);\n",
    "     assert_eq!(large[\"sealed\"], 3000);\n",
    "     assert_eq!(large[\"rewrapped\"], 0);\n",
    "+    sharing_task12_benchmark(test, owner, reader).await;\n",
    "     for slug in slugs {\n",
    "-        if slug == \"large\" {\n",
    "+        if slug == \"large\" || slug.starts_with(\"benchmark-\") {\n",
    "             let response = reqwest::Client::builder()\n",
    "                 .danger_accept_invalid_certs(true)\n",
    "                 .timeout(std::time::Duration::from_secs(180))\n",
    "@@ -2895,6 +2906,309 @@\n",
    "         .request(\"DELETE\", &format!(\"{HOME}default/\"), \"\")\n",
    "         .await;\n",
    "     test.wait_for_tasks().await;\n",
    "+}\n",
    "+\n",
    "+// Temporary Task 12 qualification instrumentation; retain the patch/evidence and restore source.\n",
    "+async fn sharing_task12_benchmark(test: &TestServer, owner: u32, reader: u32) {\n",
    "+    use crate::utils::za::{plant_event, plant_sealed_event};\n",
    "+    use serde_json::json;\n",
    "+    use std::time::{Duration, Instant};\n",
    "+    const USER: &str = \"share-http-races@example.com\";\n",
    "+    const READER: &str = \"shares-http-reader@example.com\";\n",
    "+    const HOME: &str = \"/dav/cal/share-http-races%40example.com/\";\n",
    "+    let http = reqwest::Client::builder()\n",
    "+        .danger_accept_invalid_certs(true)\n",
    "+        .timeout(Duration::from_secs(180))\n",
    "+        .build()\n",
    "+        .unwrap();\n",
    "+    for slug in [\"benchmark-full\", \"benchmark-resume\"] {\n",
    "+        let calendar_id = test\n",
    "+            .server\n",
    "+            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)\n",
    "+            .await\n",
    "+            .unwrap()\n",
    "+            .by_path(slug)\n",
    "+            .unwrap()\n",
    "+            .document_id();\n",
    "+        let mut expected_ids = std::collections::BTreeSet::new();\n",
    "+        let mut original_kinds = BTreeMap::new();\n",
    "+        let mut original_bodies = BTreeMap::new();\n",
    "+        // Exactly 1,500 EWK-sealed and 1,500 legacy plaintext rows, alternating input order.\n",
    "+        for index in 0..3000 {\n",
    "+            let name = format!(\"event-{index}.ics\");\n",
    "+            let uid = format!(\"task12-{slug}-{index}\");\n",
    "+            let body = super::sharing::sharing_event(&uid);\n",
    "+            let id = if index % 2 == 0 {\n",
    "+                plant_sealed_event(test, owner, slug, &name, &body, STRONG, &Default::default())\n",
    "+                    .await\n",
    "+            } else {\n",
    "+                plant_event(test, owner, slug, &name, &body, None).await\n",
    "+            };\n",
    "+            assert!(expected_ids.insert(id));\n",
    "+            original_kinds.insert(id, if index % 2 == 0 { 1 } else { 0 });\n",
    "+            let served =\n",
    "+                task12_benchmark_get(&http, USER, &format!(\"{HOME}{slug}/{name}\"), 200).await;\n",
    "+            let served_uids: Vec<_> = served\n",
    "+                .lines()\n",
    "+                .filter_map(|line| line.strip_prefix(\"UID:\"))\n",
    "+                .collect();\n",
    "+            assert_eq!(served_uids, vec![uid.as_str()]);\n",
    "+            assert!(original_bodies.insert(name, served).is_none());\n",
    "+        }\n",
    "+        // This complete store walk detects extra rows/links as well as missing expected ids.\n",
    "+        let baseline = task12_benchmark_snapshot(test, owner, calendar_id).await;\n",
    "+        assert_eq!(\n",
    "+            baseline\n",
    "+                .keys()\n",
    "+                .copied()\n",
    "+                .collect::<std::collections::BTreeSet<_>>(),\n",
    "+            expected_ids\n",
    "+        );\n",
    "+        assert_eq!(baseline.len(), 3000);\n",
    "+        for (id, (kind, _)) in &baseline {\n",
    "+            assert_eq!(*kind, original_kinds[id]);\n",
    "+        }\n",
    "+        // Bob has his own credentials, no grant yet, and cannot read an owner row.\n",
    "+        task12_benchmark_get(&http, READER, &format!(\"{HOME}{slug}/event-0.ics\"), 403).await;\n",
    "+        let body = json!({\"username\":USER,\"password\":STRONG,\"calendar\":slug,\n",
    "+            \"address\":READER,\"level\":\"view\"});\n",
    "+        let grant_workflow_started = Instant::now();\n",
    "+        let mut committed_rewrapped = 0_u64;\n",
    "+        let mut committed_sealed = 0_u64;\n",
    "+        let mut interrupted_ms = 0_u128;\n",
    "+        if slug == \"benchmark-resume\" {\n",
    "+            // First park: phase 1 acknowledged, before the full parentage walk.\n",
    "+            let first = HookParked::start(\n",
    "+                owner,\n",
    "+                test_hooks::set_share_rewrap_pause,\n",
    "+                task12_benchmark_grant(http.clone(), body.clone()),\n",
    "+            )\n",
    "+            .await;\n",
    "+            // Second park: first 100 event writes acknowledged, next group loaded, no next write.\n",
    "+            // HookParked arms this slot before first.finish() releases phase 1.\n",
    "+            let second =\n",
    "+                HookParked::start(owner, test_hooks::set_share_rewrap_pause, first.finish()).await;\n",
    "+            test_hooks::set_share_rewrap_fail_once(Some(owner));\n",
    "+            // Do not read/verify while timed HTTP work is parked.\n",
    "+            let (reply, elapsed) = second.finish().await;\n",
    "+            interrupted_ms = elapsed.as_millis();\n",
    "+            assert_eq!(reply.status, 500);\n",
    "+            println!(\n",
    "+                \"TASK12_BENCH {}\",\n",
    "+                json!({\"calendar\":slug,\"attempt\":\"interrupted\",\n",
    "+                \"status\":reply.status,\"response\":reply.body,\"request_ms\":interrupted_ms,\n",
    "+                \"induced_pause\":true})\n",
    "+            );\n",
    "+            let journal = task12_benchmark_ring(test, owner, calendar_id).await;\n",
    "+            assert!(!journal.converted && !journal.reverting && journal.grants.is_empty());\n",
    "+            let partial = task12_benchmark_snapshot(test, owner, calendar_id).await;\n",
    "+            assert_eq!(\n",
    "+                partial\n",
    "+                    .keys()\n",
    "+                    .copied()\n",
    "+                    .collect::<std::collections::BTreeSet<_>>(),\n",
    "+                expected_ids\n",
    "+            );\n",
    "+            for (id, (kind, _)) in &partial {\n",
    "+                if *kind == 3 {\n",
    "+                    match original_kinds[id] {\n",
    "+                        0 => committed_sealed += 1,\n",
    "+                        1 => committed_rewrapped += 1,\n",
    "+                        _ => unreachable!(),\n",
    "+                    }\n",
    "+                } else {\n",
    "+                    assert_eq!(*kind, original_kinds[id]);\n",
    "+                }\n",
    "+            }\n",
    "+            assert_eq!(committed_rewrapped + committed_sealed, 100);\n",
    "+            // No previous request counts are fabricated from the 500 problem body.\n",
    "+            println!(\n",
    "+                \"TASK12_BENCH {}\",\n",
    "+                json!({\"calendar\":slug,\"checkpoint\":\"after500\",\n",
    "+                \"committed_envelope_rewrites\":committed_rewrapped,\n",
    "+                \"committed_plaintext_seals\":committed_sealed,\"inventory\":partial.len(),\n",
    "+                \"converted\":false,\"reverting\":false,\"grants\":0})\n",
    "+            );\n",
    "+            for (_, (_, event)) in &partial {\n",
    "+                let name = &event.names[0].name;\n",
    "+                let served =\n",
    "+                    task12_benchmark_get(&http, USER, &format!(\"{HOME}{slug}/{name}\"), 200).await;\n",
    "+                assert_eq!(&served, &original_bodies[name]);\n",
    "+            }\n",
    "+            task12_benchmark_get(&http, READER, &format!(\"{HOME}{slug}/event-0.ics\"), 403).await;\n",
    "+        }\n",
    "+        // Timer is fresh: setup, first-attempt checkpoint and verification reads are outside it.\n",
    "+        let (reply, elapsed) = task12_benchmark_grant(http.clone(), body).await;\n",
    "+        let workflow_elapsed = grant_workflow_started.elapsed();\n",
    "+        assert_eq!(reply.status, 200);\n",
    "+        assert_eq!(reply.body[\"ok\"], true);\n",
    "+        assert_eq!(reply.body[\"changed\"], true);\n",
    "+        assert_eq!(reply.body[\"rewrapped\"], 1500 - committed_rewrapped);\n",
    "+        assert_eq!(reply.body[\"sealed\"], 1500 - committed_sealed);\n",
    "+        println!(\n",
    "+            \"TASK12_BENCH {}\",\n",
    "+            json!({\"calendar\":slug,\n",
    "+            \"attempt\":if slug == \"benchmark-full\" {\"first-grant\"} else {\"resume\"},\n",
    "+            \"status\":reply.status,\"response\":reply.body,\"request_ms\":elapsed.as_millis(),\n",
    "+            \"sum_http_request_ms\":interrupted_ms + elapsed.as_millis(),\n",
    "+            \"workflow_wall_ms_including_checkpoint_reads\":workflow_elapsed.as_millis(),\n",
    "+            \"initial_ewk_sealed\":1500,\"initial_legacy_plaintext\":1500,\n",
    "+            \"logical_group_limit\":100,\"physical_event_commit\":\"one event\",\n",
    "+            \"in_process_test_server\":true,\"debug_assertions\":cfg!(debug_assertions)})\n",
    "+        );\n",
    "+        let ring = task12_benchmark_ring(test, owner, calendar_id).await;\n",
    "+        assert!(ring.converted && !ring.reverting);\n",
    "+        assert_eq!(ring.grants.len(), 1);\n",
    "+        assert_eq!(ring.grants[0].account_id, reader);\n",
    "+        let final_rows = task12_benchmark_snapshot(test, owner, calendar_id).await;\n",
    "+        assert_eq!(\n",
    "+            final_rows\n",
    "+                .keys()\n",
    "+                .copied()\n",
    "+                .collect::<std::collections::BTreeSet<_>>(),\n",
    "+            expected_ids\n",
    "+        );\n",
    "+        let keys = za_unlock(test, owner, STRONG).await;\n",
    "+        let context = za_owned_calendar_context(&test.server, &keys, calendar_id)\n",
    "+            .await\n",
    "+            .unwrap();\n",
    "+        let mut seen_names = std::collections::BTreeSet::new();\n",
    "+        let mut seen_uids = std::collections::BTreeSet::new();\n",
    "+        for (id, (kind, mut event)) in final_rows {\n",
    "+            assert_eq!(kind, 3);\n",
    "+            unseal_event_in_domain(&mut event, &context.domain).unwrap();\n",
    "+            let name = &event.names[0].name;\n",
    "+            assert!(seen_names.insert(name.clone()));\n",
    "+            let uid = event.data.event.uids().next().unwrap().to_string();\n",
    "+            assert!(seen_uids.insert(uid));\n",
    "+            let old = &baseline[&id].1;\n",
    "+            assert_eq!(event.names, old.names);\n",
    "+            assert_eq!(event.created, old.created);\n",
    "+            assert_eq!(event.modified, old.modified);\n",
    "+            assert_eq!(event.schedule_tag, old.schedule_tag);\n",
    "+            assert_eq!(event.flags, old.flags);\n",
    "+            let owner_body =\n",
    "+                task12_benchmark_get(&http, USER, &format!(\"{HOME}{slug}/{name}\"), 200).await;\n",
    "+            assert_eq!(&owner_body, &original_bodies[name]);\n",
    "+            let bob_body =\n",
    "+                task12_benchmark_get(&http, READER, &format!(\"{HOME}{slug}/{name}\"), 200).await;\n",
    "+            assert_eq!(bob_body, owner_body);\n",
    "+        }\n",
    "+        assert_eq!(seen_names.len(), 3000);\n",
    "+        assert_eq!(seen_uids.len(), 3000);\n",
    "+        assert_eq!(seen_names, original_bodies.keys().cloned().collect());\n",
    "+        println!(\n",
    "+            \"TASK12_BENCH {}\",\n",
    "+            json!({\"calendar\":slug,\"verification\":\"passed\",\n",
    "+            \"exact_id_inventory\":3000,\"unique_names\":3000,\"unique_uids\":3000,\n",
    "+            \"alice_exact_body_matches\":3000,\"bob_own_credential_exact_body_matches\":3000})\n",
    "+        );\n",
    "+    }\n",
    "+}\n",
    "+\n",
    "+async fn task12_benchmark_grant(\n",
    "+    http: reqwest::Client,\n",
    "+    body: serde_json::Value,\n",
    "+) -> (crate::utils::za::VaultReply, std::time::Duration) {\n",
    "+    let request = http\n",
    "+        .post(format!(\n",
    "+            \"{}/api/vault/shares/grant\",\n",
    "+            crate::utils::za::SERVER_URL\n",
    "+        ))\n",
    "+        .json(&body);\n",
    "+    let started = std::time::Instant::now();\n",
    "+    let response = request.send().await.unwrap();\n",
    "+    let status = response.status().as_u16();\n",
    "+    let body = response.json().await.unwrap();\n",
    "+    let elapsed = started.elapsed();\n",
    "+    (crate::utils::za::VaultReply { status, body }, elapsed)\n",
    "+}\n",
    "+\n",
    "+async fn task12_benchmark_get(\n",
    "+    http: &reqwest::Client,\n",
    "+    user: &str,\n",
    "+    path: &str,\n",
    "+    status: u16,\n",
    "+) -> String {\n",
    "+    let response = http\n",
    "+        .get(format!(\"{}{path}\", crate::utils::za::SERVER_URL))\n",
    "+        .basic_auth(user, Some(STRONG))\n",
    "+        .send()\n",
    "+        .await\n",
    "+        .unwrap();\n",
    "+    assert_eq!(response.status().as_u16(), status);\n",
    "+    response.text().await.unwrap()\n",
    "+}\n",
    "+\n",
    "+// Raw envelope type is metadata, never emit an envelope or key. Strict classifier first.\n",
    "+async fn task12_benchmark_snapshot(\n",
    "+    test: &TestServer,\n",
    "+    owner: u32,\n",
    "+    calendar_id: u32,\n",
    "+) -> BTreeMap<u32, (u8, CalendarEvent)> {\n",
    "+    use groupware::calendar::seal::{ZaEventStorageKind, classify_event_storage};\n",
    "+    let mut rows = BTreeMap::new();\n",
    "+    test.server\n",
    "+        .all_archives(\n",
    "+            owner,\n",
    "+            Collection::CalendarEvent,\n",
    "+            types::field::Field::ARCHIVE.into(),\n",
    "+            |id, stored| {\n",
    "+                let event = stored.deserialize::<CalendarEvent>()?;\n",
    "+                if event.names.iter().any(|name| name.parent_id == calendar_id) {\n",
    "+                    assert_eq!(event.names.len(), 1);\n",
    "+                    let kind = match classify_event_storage(&event).unwrap() {\n",
    "+                        ZaEventStorageKind::LegacyPlaintext => 0,\n",
    "+                        ZaEventStorageKind::Sealed => {\n",
    "+                            let text = event.data.event.to_string().replace(\"\\r\\n \", \"\");\n",
    "+                            let encoded = text\n",
    "+                                .lines()\n",
    "+                                .find_map(|line| line.strip_prefix(\"X-ZA-KEY:\"))\n",
    "+                                .unwrap();\n",
    "+                            STANDARD.decode(encoded).unwrap()[1]\n",
    "+                        }\n",
    "+                    };\n",
    "+                    assert!(rows.insert(id, (kind, event)).is_none());\n",
    "+                }\n",
    "+                Ok(())\n",
    "+            },\n",
    "+        )\n",
    "+        .await\n",
    "+        .unwrap();\n",
    "+    rows\n",
    "+}\n",
    "+\n",
    "+async fn task12_benchmark_ring(test: &TestServer, owner: u32, calendar_id: u32) -> ZaOwnerRing {\n",
    "+    let keys = za_unlock(test, owner, STRONG).await;\n",
    "+    let context = za_owned_calendar_context(&test.server, &keys, calendar_id)\n",
    "+        .await\n",
    "+        .unwrap();\n",
    "+    let calendar = context.stored.deserialize::<Calendar>().unwrap();\n",
    "+    let entry = calendar\n",
    "+        .preferences\n",
    "+        .iter()\n",
    "+        .find(|entry| entry.account_id == owner)\n",
    "+        .unwrap();\n",
    "+    let encoded = entry\n",
    "+        .name\n",
    "+        .strip_prefix(\"$za$\")\n",
    "+        .unwrap()\n",
    "+        .split('|')\n",
    "+        .next()\n",
    "+        .unwrap();\n",
    "+    let ring = groupware::calendar::sharing::format::open_owner_ring(\n",
    "+        &STANDARD.decode(encoded).unwrap(),\n",
    "+        owner,\n",
    "+        calendar_id,\n",
    "+        keys.ewk(),\n",
    "+    )\n",
    "+    .unwrap()\n",
    "+    .1;\n",
    "+    assert_eq!(calendar.acls.len(), ring.grants.len());\n",
    "+    if ring.grants.is_empty() {\n",
    "+        assert_eq!(calendar.preferences.len(), 1);\n",
    "+    }\n",
    "+    ring\n",
    " }\n",
    " \n",
    " async fn sharing_terminal_contracts(test: &TestServer, owner: u32, client: &DummyWebDavClient) {\n"
  ]
}
```

## Appendix D: machine and restoration receipts

Initial sandbox hardware-sysctl access was refused; the authorized non-identifying read
succeeded with local listener permission. The setup-only failed invocation is preserved
separately from the successful run. Source restoration and command results:

```json
{
  "benchmark_commands": [
    {
      "name": "benchmark-compile",
      "command": [
        "cargo",
        "test",
        "-p",
        "tests",
        "za::za_tests",
        "--no-run"
      ],
      "exit": 0,
      "elapsed_seconds": 55.543282985687256,
      "started_unix": 1792873708.740452,
      "environment": {}
    },
    {
      "name": "benchmark-run",
      "command": [
        "cargo",
        "test",
        "-p",
        "tests",
        "za::za_tests",
        "--",
        "--nocapture"
      ],
      "exit": 101,
      "elapsed_seconds": 7.997143745422363,
      "started_unix": 1792873773.816312,
      "environment": {}
    },
    {
      "name": "benchmark-run-2",
      "command": [
        "cargo",
        "test",
        "-p",
        "tests",
        "za::za_tests",
        "--",
        "--nocapture"
      ],
      "exit": 0,
      "elapsed_seconds": 719.4411039352417,
      "started_unix": 1792873790.825356,
      "environment": {
        "STORE": "RocksDb",
        "RUST_MIN_STACK": "16777216"
      }
    }
  ],
  "machine": [
    {
      "command": [
        "uname",
        "-s",
        "-r",
        "-m"
      ],
      "exit": 0,
      "stdout": "Darwin 27.0.0 arm64\n",
      "stderr": ""
    },
    {
      "command": [
        "sw_vers"
      ],
      "exit": 0,
      "stdout": "ProductName:\t\tmacOS\nProductVersion:\t\t27.0.1\nBuildVersion:\t\t26A434\n",
      "stderr": ""
    }
  ],
  "toolchain_hardware_conditions": {
    "commands": [
      {
        "command": [
          "rustc",
          "--version",
          "--verbose"
        ],
        "exit": 0,
        "stdout": "rustc 1.99.0 (b940084d7 2026-09-28)\nbinary: rustc\ncommit-hash: b940084d7eb6a299eb4bfeb8e34901bc051e7ac4\ncommit-date: 2026-09-28\nhost: aarch64-apple-darwin\nrelease: 1.99.0\nLLVM version: 23.1.1\n",
        "stderr": ""
      },
      {
        "command": [
          "cargo",
          "--version"
        ],
        "exit": 0,
        "stdout": "cargo 1.99.0 (5f94df478 2026-08-27)\n",
        "stderr": ""
      },
      {
        "command": [
          "sysctl",
          "-n",
          "hw.model",
          "hw.physicalcpu",
          "hw.logicalcpu",
          "hw.memsize"
        ],
        "exit": 0,
        "stdout": "Mac13,1\n10\n10\n68719476736\n",
        "stderr": ""
      }
    ],
    "conditions": {
      "STORE": "RocksDb",
      "RUST_MIN_STACK": 16777216,
      "profile": "test opt-level=0, debuginfo=1; Argon2 opt-level=3",
      "features": "tests defaults rocks+sqlite; test_mode/enterprise test dependencies",
      "NO_INSERT": "unset",
      "ZA_KEY_ACCOUNTS": "unset",
      "DRAIN_SKIP_ALL": "suite default",
      "http_timeout_seconds": 180,
      "binary": "in-process tests server, TLS loopback",
      "cache": "seed/baseline reads warm owner paths and storage",
      "competing_build": "none by this executor"
    }
  },
  "profile_environment": {
    "BLOB_STORE": null,
    "NO_INSERT": null,
    "ZA_KEY_ACCOUNTS": null,
    "RUSTFLAGS": null,
    "CARGO_ENCODED_RUSTFLAGS": null,
    "CARGO_PROFILE_TEST_OPT_LEVEL": null,
    "CARGO_PROFILE_TEST_DEBUG": null,
    "CARGO_PROFILE_RELEASE_LTO": null,
    "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": null,
    "SEARCH_STORE": null,
    "MEMORY_STORE": null
  },
  "restoration": {
    "original_sha256": "50004e1c6104a550c21cf1ef5c327713fac5aa7950fc45189afba1f6b33eb9aa",
    "applied_source_sha256": "336e1a9b9829ad664160f886fe5b818f76164663cffa309c1f4d4a076c31f8a8",
    "restored_sha256": "50004e1c6104a550c21cf1ef5c327713fac5aa7950fc45189afba1f6b33eb9aa",
    "formatted_patch_sha256": "050ec1f1bf53f2e9278288743ab601cb0552f16cb57b294792609d008be7d42b",
    "pre_status": "?? docs/superpowers/specs/2026-10-20-zero-access-mail-design-review.md\n",
    "restored_status": "?? docs/superpowers/specs/2026-10-20-zero-access-mail-design-review.md\n",
    "exact_status_equal": true,
    "tracked_source_diff_empty": true
  },
  "final_gate_receipts": [
    {
      "gate": "final-format",
      "command": "cargo fmt --all --check",
      "exit": 0,
      "elapsed_seconds": 3.3916988372802734,
      "test_result_lines": []
    },
    {
      "gate": "final-units",
      "command": "cargo test -p vault -p groupware -p common",
      "exit": 0,
      "elapsed_seconds": 9.59600281715393,
      "test_result_lines": [
        "test result: ok. 121 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s",
        "test result: ok. 146 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s",
        "test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
      ]
    },
    {
      "gate": "final-http-units",
      "command": "cargo test -p http@0.16.26 --features test_mode",
      "exit": 0,
      "elapsed_seconds": 29.009573936462402,
      "test_result_lines": [
        "test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
      ]
    },
    {
      "gate": "final-za",
      "command": "STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture",
      "exit": 0,
      "elapsed_seconds": 392.5907027721405,
      "test_result_lines": [
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 354.82s"
      ]
    },
    {
      "gate": "final-webdav",
      "command": "STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests",
      "exit": 0,
      "elapsed_seconds": 17.87021803855896,
      "test_result_lines": [
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 16.96s"
      ]
    },
    {
      "gate": "final-key-webdav",
      "command": "STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests",
      "exit": 0,
      "elapsed_seconds": 21.812842845916748,
      "test_result_lines": [
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 21.30s"
      ]
    },
    {
      "gate": "final-key-jmap",
      "command": "STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests",
      "exit": 0,
      "elapsed_seconds": 142.57461786270142,
      "test_result_lines": [
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 97 filtered out; finished in 142.03s"
      ]
    },
    {
      "gate": "final-product-build",
      "command": "cargo build --release -p stalwart --no-default-features --features rocks",
      "exit": 0,
      "elapsed_seconds": 1145.865168094635,
      "test_result_lines": []
    }
  ]
}
```
