# Stalwart 1.0 upgrade: independent Codex review

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
