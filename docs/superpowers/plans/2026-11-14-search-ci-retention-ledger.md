# Search CI retention ledger

Status: primary and independent source review confirmed the mappings and authorized subject
retirements on 2026-11-14. The eight new dispositions and two exact Keyless moves in
[`tests/qualification/dispositions.json`](../../../tests/qualification/dispositions.json)
are `unconfirmed: false`. Both IMAP and JMAP restorations passed independent source review.
Both complete local key-mode protocol suites passed. Fresh hosted strict-rule execution
remains pending; local suite success is not a strict-rule count claim.

The source inspection started at `81ac842507ae098b42bd5a72996c43f882bdba03`, with the
separately reviewed corpus-setup merge pending in the working tree. The historical inventory
is `baseline-0.16.27.json`, measured at `5b914ddb5`. The immediate pre-replacement source is
`070c1d3a4^` for the IMAP/JMAP variants and `e5d3e749e^` for the mail search replacements.
The final integrated source revision and fresh results must be recorded before acceptance.

The hosted candidate retention job `132176748499` of run `43509717996` reported 25 inventory
failures: one missing libtest and 24 function-step failures. All function failures said no
body record existed in any step export; the named old identities were absent from that
candidate's source before these restorations.
The affected `imap-key`, `jmap-key`, `za-tests` and `za-tests-fs-blob` net steps exited zero,
and the coverage gate passed. The local receipt is
[candidate-retention.log](../../../.superpowers/sdd/2026-11-13-search-ci-curve-profile/hosted/candidate-retention.log).
The receipt does not include `functions.json` or raw profiles. It cannot establish that a
successor satisfies the strict execution rule.

## Retained contract

- [C10](../specs/2026-10-27-stalwart-1-upgrade-design.md#test-suite-ownership) keeps each
  baseline test and helper required in every original step, unless a recorded disposition
  applies. The body, last statement and closing-brace counts must all be positive.
- [Search design section 9.3](../specs/2026-11-04-zero-access-search-design.md#93-variants-that-narrow)
  changes the keyed variants from content refusals and singleton wire identities to
  upstream search/thread outcomes, with keyed currentness and separate keyless refusals.
- [Search design section 14](../specs/2026-11-04-zero-access-search-design.md#14-release-pointers)
  explicitly supersedes the old mail-spec content-refusal contract. It does not retire
  supported metadata filters, sorts, paging, or genuinely unsupported operations.
- The [gate schema](../../../scripts/tests/README.md#the-dispositions-file) requires named
  equivalents in the original step. `case` records document changed expectations but excuse
  nothing. Exact function moves retain the `(file, in, name)` identity shape. No baseline,
  selector, step environment, threshold or gate algorithm changes are proposed.

## Identity and expectation ledger

`ZA steps` below means both `za-tests` and `za-tests-fs-blob`. Function identities without an
explicit `in` value have `in: null`.

| Old identity | Required step | Replacement or retained requirement | Disposition |
|---|---|---|---|
| `tests/src/za/mail/gates.rs`, `<impl Keyless>`, `name` | ZA steps | `tests/src/utils/za.rs`, `<impl Keyless>`, `name`; the same Bearer/WebSocket labels | Source-confirmed exact function move, commit `df9ffaf44`; fresh execution pending |
| `tests/src/za/mail/gates.rs`, `<impl Keyless>`, `call` | ZA steps | `tests/src/utils/za.rs`, `<impl Keyless>`, `call`; the same channel request/response helper | Source-confirmed exact function move, commit `df9ffaf44`; fresh execution pending |
| `tests/src/za/mail/reads.rs::threads_are_singletons` | ZA steps | Both `reads.rs::singleton_rows_have_mapped_wire_ids` and `threads/task12.rs::case_conversation_parity` | `search-r7-persisted-singletons-mapped-wire-ids`, equivalent functions |
| `imap::imap_tests -> za_variants::thread` | `imap-key` | `tests/src/imap/thread.rs::test` | `search-r7-imap-thread-upstream-outcomes`, equivalent sub-test call |
| `imap::imap_tests -> za_variants::search` | `imap-key` | Restored served-metadata assertions in the original variant identity, retaining the upstream positive content search body | No excusal or mapping; source restoration reviewed, fresh execution pending |
| `tests/src/imap/za_variants.rs::sizes_by_uid` | `imap-key` | Restored independent RFC822.SIZE readback helper with its size-sort caller | No excusal; source restoration reviewed, fresh execution pending |
| `tests/src/imap/za_variants.rs::assert_search_refused` | `imap-key` | Retire only the assertion that supported keyed content commands always return `NO [CANNOT]` with `ZA_MAIL_SEARCH_REFUSED` | `search-r7-imap-content-refusal-assertion-retired`, deleted subject |
| `tests/src/jmap/za_variants.rs::date` | `jmap-key` | Restored support for the retained metadata oracle | No excusal; source restoration reviewed, fresh execution pending |
| `tests/src/jmap/za_variants.rs::email_query` | `jmap-key` | Restored metadata query request helper | No excusal; source restoration reviewed, fresh execution pending |
| `tests/src/jmap/za_variants.rs::ids` | `jmap-key` | Restored response-type and returned-ID helper | No excusal; source restoration reviewed, fresh execution pending |
| `tests/src/jmap/za_variants.rs::load_corpus` | `jmap-key` | Restored metadata readback and exact ID completeness, after keyed content currentness | No excusal; source restoration reviewed, fresh execution pending |
| `tests/src/jmap/za_variants.rs::query_served` | `jmap-key` | Restored corpus-derived metadata filter/sort oracle, adapting mapped threading and collapse | No excusal; source restoration reviewed, fresh execution pending |
| `tests/src/jmap/za_variants.rs::query_options` | `jmap-key` | Restored metadata-only `receivedAt` paging alongside upstream content-sort paging | No excusal; source restoration reviewed, fresh execution pending |
| `tests/src/jmap/za_variants.rs::query_refusals` | `jmap-key` | Restored function with an inline assertion for the surviving `cc` unsupported-sort case; only supported-content refusal cases retire | `search-r7-jmap-supported-content-refusal-cases-retired` documents cases only and cannot excuse the function; source restoration reviewed, fresh execution pending |
| `tests/src/jmap/za_variants.rs::assert_search_refused` | `jmap-key` | Retire the helper's obsolete exact `ZA_MAIL_SEARCH_REFUSED` error-description contract; do not retire the surviving `cc` refusal | `search-r7-jmap-content-refusal-assertion-retired`, deleted subject |
| `tests/src/za/mail/search.rs::imap_refused_keys` | ZA steps | Both `imap_served_content_keys` and `mail/differential.rs::compare_imap`: exact positive content results, sort parity, BODY- and FROM-filtered THREAD groups, unsupported CC sort and trace checks | `search-r7-imap-content-commands-served`, both equivalent functions required |
| `tests/src/za/mail/search.rs::refused_commands` | ZA steps | Command cases are split between `imap_served_content_keys` and `mail/differential.rs::compare_imap`; the latter preserves FROM-filtered THREAD | Same two-function equivalent entry; no deleted-subject waiver |
| `tests/src/za/mail/search.rs::jmap_refused_filters_and_sorts` | ZA steps | `jmap_served_content_filters_and_sorts`: positive content, queryChanges and snippets, retaining metadata refusal | `search-r7-jmap-content-filters-sorts-served`, equivalent function |
| `jmap::email::za::tests::first_snippet_term_finds_the_marked_properties_anywhere` | `unit-tests`; selected by `test.yml__unit-tests__workspace-unit-tests` | The `first_snippet_term` refusal selector and `za_refuse_snippet_terms` were deleted together in `e5d3e749e`; no unit-test rename or integration-step equivalent is claimed | `search-r7-snippet-refusal-selector-retired`, deleted internal subject |

## Source-backed replacements

The [shared Keyless implementation](../../../tests/src/utils/za.rs) preserves the old
`<impl Keyless>` methods. The old and new paths are compared in commit `df9ffaf44`; only
these two exact function renames are recorded.

The [mapped read fixture](../../../tests/src/za/mail/reads.rs) checks that persisted Email
rows retain `thread_id == document_id`, while Email/get emits exact joined IDs and a
collapsed query returns one representative. That helper alone is insufficient: it removed
the old Thread/get and IMAP THREAD assertions. Therefore the equivalent list also requires
[`case_conversation_parity`](../../../tests/src/za/mail/threads/task12.rs), which checks
exact joined membership, independent singleton groups, explicit missing thread IDs, collapse,
and IMAP REFERENCES/ORDEREDSUBJECT results. Both are reached by the ZA suite.

The [IMAP thread body](../../../tests/src/imap/thread.rs), called unconditionally by
[`imap_tests`](../../../tests/src/imap/mod.rs), uses the old Multiappend fixture. Its joined
groups and successful subject search replace intentional singleton/refusal outcomes; the
THREADID membership, EMAILID singleton selection and deletion checks remain.

The [mail search replacements](../../../tests/src/za/mail/search.rs) run from that module's
`test` function in both ZA steps. `imap_served_content_keys` covers the former content
filters and attachment keywords with exact results, supported-sort parity, BODY-filtered
THREAD groups and a no-result canary. The old `refused_commands` also included
`THREAD REFERENCES UTF-8 FROM x`; the served helper's BODY case does not replace that
filter. Therefore the equivalent list also requires
[`mail/differential.rs::compare_imap`](../../../tests/src/za/mail/differential.rs), whose
ALL/FROM/BODY THREAD loop requires nonempty ordinary results and exact projected keyed
groups. The gate must verify both functions to completion in each original ZA step.
`jmap_served_content_filters_and_sorts` covers content and
attachment filters, supported sorts, stale/current queryChanges, marked and bare snippets,
and deletion. Its `text: "!!!"` and NOT snippet cases now compare successful output with
the ordinary control; they no longer ask the removed selector to manufacture a refusal.

## Refusals that remain required

The audit distinguishes content becoming supported from an operation that remains unsupported.

| Case | Current source and requirement | Status |
|---|---|---|
| IMAP `SORT (CC)` | `mail/search.rs::imap_served_content_keys` requires `NO`, the unsupported-CC message, no SORT line, and a successful subsequent NOOP. `mail/differential.rs::compare_imap` also checks it. | Retained in ZA steps; not retired with content refusals |
| JMAP sort `cc` | `crates/jmap/src/email/query.rs::sort_comparators` still returns `unsupportedSort`. `mail/differential.rs::compare_jmap` checks both account modes, both directions, secondary sorts and collapse, with no IDs and matching refusal payloads. | ZA coverage remains; `za_variants::query_refusals` restores the inline `jmap-key` refusal assertion, with fresh execution pending |
| JMAP object metadata | `mail/search.rs::jmap_served_content_filters_and_sorts` still checks standalone `metadataExists` and a mixed expression whose metadata leaf follows supported text. It supplies the metadata capability and expects the exact key-account refusal; ordinary controls succeed. | Retained by the source-confirmed equivalent; fresh execution pending |
| Keyless mail requests | `jmap/za_variants.rs::keyless_query_refused` and `mail/gates.rs` retain the separate authorization boundary. | Not equivalents for an owner-keyed content-refusal helper |
| Temporarily unavailable content | `mail/search.rs::unavailable_refusal` retains bounded refusal when a current content view cannot be acquired. | Separate release-7 fallback; not a reason to restore unconditional content refusal |

The 1.0 expectation ledger already records removal of `cc` from
`mail/search.rs::jmap_refused_filters_and_sorts`'s sort list because ordinary accounts also
reject it ([part 7c row](2026-10-27-stalwart-1-upgrade-outcome.md#expectation-ledger)).
However, `070c1d3a4^:tests/src/jmap/za_variants.rs::query_refusals` still includes `cc`.
That is why these entries add no function-level excusal for `query_refusals`. Its restored
body checks `cc` inline; `assert_search_refused` remains deleted, so the helper's narrowly
retired fixed-description contract does not excuse the surviving operation. A
documentation-only case record covers the supported filters and sorts, their mixed queries,
and the obsolete priority between content-filter and content-sort refusals.

## Coverage restoration requirements

The IMAP supplement must preserve successful size/arrival sorting and the flag-only saved
search matrix from `070c1d3a4^:tests/src/imap/za_variants.rs::search`. Size order is checked
against independent RFC822.SIZE readback, not against another sort call. Preserve the saved
flag results, `$` size filtering, SAVE with MIN/MAX/ALL/COUNT combinations, and the
single-match case. The current upstream content-predicate saved searches stay in place.

The JMAP supplement must preserve the corpus-derived metadata cases from
`070c1d3a4^:tests/src/jmap/za_variants.rs`, including `minSize`, `maxSize`, `inThread`,
size order, receivedAt direction, keywords, mailbox predicates and metadata-only paging.
Mapped thread membership and collapse replace the old singleton assumptions; the oracle
must derive the new expectations independently rather than accepting query output as its
own expected result. Keep current content-currentness and complete per-chunk ID checks.
The [compliance suite](../../../tests/src/jmap/mod.rs) skips key mode, so its size/query
checks cannot satisfy these `jmap-key` requirements.

The protocol writers restored the original helper identities listed above. Independent
source reviews accepted both restorations, and the primary accepted the mapping and
authorized subject-retirement decisions. The subsequent local runtime results below
qualify the restored checks, while hosted strict-rule counts remain pending.

## Source review and pending execution

On 2026-11-14, independent mapping review identified a missing FROM-filtered THREAD
requirement in the initial IMAP equivalent. Requiring `compare_imap` alongside
`imap_served_content_keys` closed that finding, and the correction review approved the
mappings. The primary inspected the exact moves and composite equivalents and confirmed
all eight new dispositions and both Keyless moves. The separate independent IMAP and JMAP
source reviews also passed. After confirmation, the primary ran both complete key-mode
suites sequentially from the same frozen binary: IMAP passed in 12.02 seconds and JMAP
in 208.60 seconds. The build, formatting, workflow command check, and all 43 active gate
self-tests passed (two existing self-tests skipped). There were no suite retries.
The adjacent `2026-11-14-search-ci-retention-outcome.md` records commands, hashes and limits.

1. Run the retained coverage net/listings/strict-rule inventory on the final candidate. Verify the
   successors' body, last-statement and exit counts in every original step.
2. Record the final revision, commands, initial/retry outcomes, gate reports and any remaining
   limitations here. Existing hosted successes do not establish the new mappings.

The initial edit of this ledger and its JSON proposals used source and retained-log inspection
only. No builds, tests, probes or network requests ran in that editing task.
