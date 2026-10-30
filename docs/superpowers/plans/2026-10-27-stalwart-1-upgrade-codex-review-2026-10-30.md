# Stalwart 1.0 upgrade: independent Codex review

**Latest status:** revision 3 was re-reviewed on 2026-10-30. The ignored-test finding is
resolved, and async constructors no longer satisfy the proposed execution gate. Six P2
findings remain in the revised specification; see "Revision 3 re-review" below. The
original revision 2 review is preserved here as historical context.

Reviewed on 2026-10-30 against checkout
`e660444544648cc2cfce9e889503a61afcc71475`, design revision 2 and implementation plan
revision 2. The pinned upstream target was inspected directly at
`23b69755cb3cd8ecd952fe0443c5ac067a506184` (`v1.0.1`).

Two P2 findings concern the proposed C10 acceptance gates. Correct these before relying on
the gates to establish preservation of the retained test suite. No additional actionable
finding was confirmed in the reviewed C1–C9 source-port architecture or C11 qualification
plan. That is a static review conclusion, not implementation or runtime qualification.

## 1. [P2] Distinguish async test execution from constructing its future

**Location:** implementation plan, Task 7, lines 388–389; design, C10 gates, lines 298–300.
The plan defines an executed sub-test as a function with a positive llvm-cov execution
count, using the baseline's method. That method can accept an async sub-test whose body
never runs.

The baseline's `coverage/tools/cov_post.py:268–272,291–295` explicitly accepts either the
outer async function or its body when matching records by the definition line. Its
`coverage/steps/za-tests.json` contains separate records for
`tests/src/za/password.rs:49`:

| Record | First coverage region | Count in the recorded run |
|---|---|---:|
| Future constructor, name ending `password4test` | `49:1–49:41` | 1 |
| Async body, name containing `password4test0` | Begins at `49:42` | 1 |

These are existing measurement artifacts under
`.superpowers/sdd/baseline-01627/`, inspected without rerunning them.

**Failure scenario:** replace the awaited call at `tests/src/za/mod.rs:136` with
`let _ = password::test(&mut test);`. The constructor still executes, but its future is
dropped without polling the body. The baseline's positive-count check can therefore report
the sub-test as executed while none of its assertions run. This is a source-derived
counterexample, not an executed mutation result. The plan's proposed omitted-call self-test
would not expose this distinction. Libtest still sees the same outer `za::za_tests` test.

**Correction:** require evidence from the async body and its meaningful assertion or
completion regions, or record explicit sub-test completion. Do not let a constructor count
satisfy the execution gate. Add negative gate self-tests for a constructed-but-unawaited
future and an early return before the required assertions, alongside the omitted-call
case. Amend the design's claim that function counts alone establish execution accordingly.

## 2. [P2] Preserve existing ignored-test status in the baseline comparison

**Location:** implementation plan, Task 7, lines 382–387; design, test dispositions and
C10 gates, lines 260–269 and 295–297.

The gate rejects any required identifier that appears in the ignored listing. However,
the baseline contains six already ignored tests within the package/filter selections the
plan retains. The stated disposition list does not explicitly account for these existing
exclusions; its named "ungated as today" examples are the cluster, stress and SCIM suites.

The baseline `inventory.json` records the following six tests with an `ignore` attribute;
their attributes are also present in the current source:

| Package | Test identifier |
|---|---|
| `nlp` | `classifier::sgd::tests::text_classifier` |
| `spam-filter` | `modules::pyzor::test::send_message` |
| `tests` | `smtp::inbound::asn::asn` |
| `tests` | `smtp::inbound::milter::milter_client_test` |
| `tests` | `smtp::outbound::dane::dane_live_smtp_hosts` |
| `tests` | `webdav::cal_query::ical_roundtrip_expansion` |

These are not all obsolete subjects. For example,
`crates/nlp/src/classifier/sgd.rs:143–148` ignores a test that opens the developer-local
`/Users/me/code/playground/phishing_email.csv`; its classifier module still exists in the
pinned target. `tests/src/smtp/outbound/dane.rs:1330–1332` explicitly marks its test as
requiring outbound TCP port 25 and a DNSSEC-validating resolver. The existing workflow also
documents two ignored workspace unit tests and the ignored CalDAV expansion test.

**Failure scenario:** faithfully restore the baseline and preserve those ignores. The
unconditional ignored-identifier rule then rejects that candidate despite no loss of
previously executed coverage. Removing the ignores instead introduces environmental
requirements and execution coverage beyond the recorded baseline.

**Correction:** carry each test's baseline ignore and execution status into Task 1's
committed inventory. Require the identifiers to remain accounted for, but fail on newly
ignored tests that were previously required to run. Explicitly disposition existing ignored
tests, including whether they must continue compiling, and add a self-test proving that an
unchanged baseline passes while a newly introduced ignore fails.

## Review scope and evidence limits

The review covered both documents, repository guidance, current fork source and test/CI
definitions, recorded baseline inventory/coverage artifacts, and pinned upstream source.
Independent read-only reviewers examined the runtime contract port and the qualification
gates; the primary reviewer checked the findings against the actual files and artifacts.
The runtime did not expose a precise primary model identity sufficient to establish a
capability ceiling, so both reviewers deliberately inherited the parent model and effort.

No builds, tests, mutation runs, server probes, CI dispatches or live-store operations were
performed. Existing baseline results were inspected as historical evidence only. Neither
the design nor the plan was edited. This report is separate from the existing review
documents, which were not read or modified.

## Revision 3 re-review — 2026-10-30

Reviewed the revision 3 design and plan at
`6a5ed014bf90eb009c45d903cca0b9641ba161ea`, including their changes since the original
review. The inspected inputs have these SHA-256 hashes:

- Design: `8d24b80343b2e14b412c01a8ff74c635f025e91948c8e3226aa59b3ec8fd5ba1`.
- Plan: `819da9b2aa2db822bdad95ae7d9355155ff855005b29778702df15ff73c5cf51`.

The pinned upstream source remains `v1.0.1` at
`23b69755cb3cd8ecd952fe0443c5ac067a506184`. Target-source references below name files in
that immutable tree; fork-source references name this checkout. Document line references
are for the inspected revision 3 files.

### Previous findings

| Original finding | Revision 3 result |
|---|---|
| Async future construction counted as execution | The constructor-only false positive is addressed by plan 554–557 and design 423–432. There is a remaining synchronous-sub-test omission in the replacement rule; see R3-6. No executable gate has been implemented or run. |
| Existing ignored tests rejected | Resolved in the proposed specification: plan 543–553 and design 417–422 preserve baseline ignore status and reject new ignores. |

### R3-1. [P2] Keep metadata bookkeeping separate from encrypted property containers

**Location:** design 152–166; plan 351–356 and 520–526.

The revised decision assigns encryption keys to all four `MetadataClass` variants. However,
`Viewer` and `Owner` hold operational state, not dead-property values. Upstream writes
property containers to `Shared` or `Private`; `PrivateMetadataWrite::build` writes the
container to `Private` at `crates/common/src/storage/metadata/mod.rs:299–321`, then updates
`Viewer` and `Owner` using `merge_viewer_state` at lines 336–354.

Those two state records contain `ViewerState { change_id, containers }`. Their serializer
and decoder require exactly 12 bytes (`crates/store/src/write/metadata.rs:156–180`), and
keyless enumeration decodes them directly
(`crates/common/src/storage/metadata/read.rs:152–184`).

**Failure scenario:** applying the prescribed collection/viewer-key encryption to an
`Owner` or `Viewer` value replaces that 12-byte record with an encryption envelope. The
next state merge or viewer enumeration reports corrupted metadata. Adding decryption to
every such reader instead would require session keys in bookkeeping/background paths.

**Correction:** encrypt protected content in the `Shared` and `Private` containers. Keep
`Owner`/`Viewer` operational state in its existing upstream format, and explicitly permit
that non-content state in the metadata leak-scanner rule. Base the interface sheet on the
actual distinction between containers and state records.

### R3-2. [P2] Do not persist the protected CLASS value through privacy flags

**Location:** design 244–252; plan 369–376.

The plan requires sanitized metadata to retain the flags read by
`EventPrivacy::from_flags`. At the target, those are `EVENT_PRIVATE` and `EVENT_SECRET`
(`crates/groupware/src/calendar/mod.rs:149–150`).
`crates/groupware/src/calendar/privacy.rs:56–81` maps them directly to the event's
`CLASS:PUBLIC`, `CLASS:PRIVATE` or `CLASS:CONFIDENTIAL` classification.

The binding calendar design explicitly seals `CLASS`
(`docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md:519–529`), and the
current `seal/policy.rs` excludes it from visible properties.

**Failure scenario:** a private or confidential event has its CLASS tree entry encrypted,
but the persisted metadata bits still reveal the classification while the owner is logged
out. This contradicts the preserved visibility policy even though the literal CLASS text
does not occur in the stored tree.

**Correction:** derive privacy flags on the keyed, unsealed response view before applying
upstream's filters. Keep them out of the persisted sanitized row. If clear classification
is desired instead, settle that as an explicit visibility-policy change, with its version
and acceptance criteria, rather than as a porting detail.

### R3-3. [P2] Preserve encryption of dead-property names as well as values

**Location:** design 165–166; plan 357–361.

The revised decision leaves property names visible and invokes the calendar visibility
allowlist. The existing policy seals dead properties, and the implementation encrypts the
complete `DeadProperty`, including names, inside the extras bundle
(`crates/groupware/src/calendar/seal/event.rs:41–45,68–96`). The calendar's iCalendar
property allowlist does not define an allowlist for arbitrary DAV XML qualified names.

Upstream does not require this disclosure. Its metadata keys contain account, class,
collection and document/viewer identifiers, not property names
(`crates/store/src/write/key.rs:589–608`). Names reside inside the encoded container.

**Failure scenario:** a custom property with sensitive namespace or local-name text now
exposes that text at rest, although the same property was previously entirely encrypted.
Refusing every name absent from the iCalendar allowlist instead breaks the custom-XML
round-trip behavior expressly required by plan 359–361.

**Correction:** protect the complete dead-property entry or container while retaining the
existing custom-property behavior. Any narrower name policy or deliberate disclosure needs
an explicit compatibility and visibility decision; it does not follow from upstream's key
layout.

### R3-4. [P2] Give each restored test module a distinct source path

**Location:** design 387–390; plan 230–235.

The exact recipe `#[path = "restored_tests.rs"] mod tests;` next to each counterpart
collides when source modules share a directory. For example, upstream 0.16.27 has distinct
inline test modules in `crates/common/src/auth/oauth/token.rs:214` and
`client_id.rs:130`. Both files remain siblings in 1.0.1. Each recipe therefore resolves
to the same `crates/common/src/auth/oauth/restored_tests.rs`.

The two original bodies use different parent-private types and helpers via `super::*`.
One shared file cannot restore both bodies verbatim under their respective parents;
overwriting it loses a module, while combining the bodies creates invalid imports/types.

**Correction:** retain the module name `tests`, but allocate a unique path per subject,
such as `#[path = "token/restored_tests.rs"] mod tests;` and
`#[path = "client_id/restored_tests.rs"] mod tests;`, or use distinct filenames. Inventory
the source-to-restored-path mapping in Task 1.

### R3-5. [P2] Exclude restored upstream tests from the fork coverage cohort

**Location:** design 433–435; plan 558–561, together with Task 2's restoration rule.

The revised fork-owned definition includes every file added relative to `v1.0.1`. All the
new `restored_tests.rs` files created by Task 2 satisfy that definition. They live under
`crates/`, so applying the baseline's classification also counts them as product code
(`coverage/tools/cov_post.py:105–128,215–225` under the recorded baseline directory).

The 96.4% reference was measured over the original 40 fork-owned files, without these
separately restored upstream test bodies.

**Failure scenario:** ported upstream unit tests run and cover their own new source files.
Their high coverage raises the candidate's fork-owned numerator and denominator, allowing
lost coverage in the actual zero-access implementation to be offset by copied test code.
The headline can stay above 96.4% while the original fork cohort loses coverage.

**Correction:** freeze and map the original fork-owned implementation cohort across moves,
account explicitly for new fork runtime code, and classify restored upstream tests
separately. Changing the upstream comparison ref to 1.0.1 is appropriate, but it must not
change the meaning of the coverage threshold by adding copied upstream tests to that group.

### R3-6. [P2] Define completion evidence for synchronous sub-tests too

**Location:** design 423–432; plan 155–159 and 554–557.

The replacement execution rule requires an async-body record for every sub-test, including
when recomputing the baseline. At least two existing sub-tests are synchronous:

- `tests/src/imap/body_structure.rs:22`, called without `.await` at
  `tests/src/imap/mod.rs:226`.
- `tests/src/webdav/cal_itip.rs:42`, called without `.await` at
  `tests/src/webdav/mod.rs:236`.

The stored `imap-plain`, `imap-key`, `webdav-plain` and `webdav-key` coverage exports show
positive ordinary-function and final-region counts for these calls. They have no separate
async-body record.

**Failure scenario:** Task 1 recomputes "ran" literally under the new rule and marks
these previously exercised calls unexecuted. Task 7 then has no baseline execution
requirement for them, so removing either inner call can evade the preservation comparison.
Alternatively, rejecting their valid ordinary records makes the unchanged-baseline
self-test fail.

**Correction:** distinguish synchronous and asynchronous functions in the inventory. For
async calls, require body/completion evidence and exclude the constructor. For synchronous
calls, use the ordinary function's body/completion regions. Add a positive synchronous
baseline fixture and a negative omitted-synchronous-call fixture to the gate self-tests.

### Re-review limits

Both independent reviewers inspected the current revision 3 files and relevant immutable
upstream source. The primary reviewer adjudicated all six findings against source and the
stored baseline exports. The two reviewed input hashes were unchanged at adjudication.

No builds, tests, mutation runs, probes, Git integration operations or live-store work were
performed for this re-review. Reading recorded coverage JSON is historical/static evidence,
not a fresh test result. Only this report was edited; the updated design, plan and other
review report were left untouched.
