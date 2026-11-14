# Search CI retention repair outcome

The hosted profile correction passed both ZA storage modes, but its Inventory gate
reported 25 absent retained identities. The repair records exact moves and approved
search expectation changes and restores the still-valid metadata checks that had
been removed with the old refusal variants. No baseline, gate algorithm, selector,
retry policy or coverage threshold changed. The adjacent retention ledger records
each original identity, step and replacement; fresh hosted execution remains the
acceptance gate for those mappings.

IMAP now runs the former flag-only SAVE/$ and SIZE/ARRIVAL cases after its existing
upstream content-search tests. SIZE has an independent RFC822.SIZE/UID oracle and
complete fixture accounting. ARRIVAL retains the old successful COUNT/ALL assertion,
without claiming an exact arrival-order oracle.

JMAP retains the upstream content cases and adds back its independent 978-message
metadata oracle: 16 filter cases in both directions with/without collapse, size and
keyword/thread-keyword sorts, and receivedAt pagination/anchors. Mapped thread
membership replaces singleton assumptions, with fixture-year/Subject/References
checks. The still-unsupported CC comparator is checked in the original jmap-key step.

Separate independent source reviews approved both protocol restorations. The mapping
review found one missing requirement: FROM-filtered THREAD lived in compare_imap,
outside the initially named replacement. The final disposition requires both that
function and imap_served_content_keys in each original ZA step. A focused correction
review closed the finding. Primary and independent source review confirmed all eight
new dispositions and both Keyless moves; they are not unconfirmed proposals.

## Fresh local validation

The affected test target compiled in 197.28 seconds. Both complete suites ran
sequentially from one frozen binary with `STORE=RocksDb`,
`RUST_MIN_STACK=16777216`, `RUST_BACKTRACE=1`, `ZA_KEY_ACCOUNTS=1`, no other `ZA_*`
selector, and no filesystem blob override:

| Libtest invocation | Result | Libtest time |
| --- | --- | ---: |
| `imap::imap_tests --exact --nocapture` | Passed, first attempt | 12.02 s |
| `jmap::jmap_tests --exact --nocapture` | Passed, first attempt | 208.60 s |

Workspace formatting, diff hygiene and the workflow command comparison passed.
Gate self-tests ran 45 cases: 43 passed and two existing cases skipped. The final
confirmed JSON was used for the last self-test run. These are local checks, not
the hosted coverage-net or strict-rule execution counts. Ordinary-mode suites
were not repeated locally for these key-mode-only test additions; final CI still
covers ordinary protocols.

All 12 recorded Rust/configuration source hashes and the frozen binary were
reverified against the completed receipts. Binary SHA-256:
`c7ca7f99f3f6105ee608cf4f3bd003c58c835f7a8f3aa73798c5ddab23b5b2ef`.
IMAP log SHA-256: `2f1a32364ce55042b2693a2058c87ff4a24ac232b8d422110704758fcdcffeb6`.
JMAP log SHA-256: `119d6db08bf5f135f9cee0dabd81386db3d75c305dbf868506cc821ee3e2bff2`.
Evidence lives under `.superpowers/sdd/2026-11-14-search-ci-retention/` in the
combined checkout. Agents deliberately inherited the parent model/effort because
its capability ceiling was unresolved; no named-model or top-tier review is claimed.

The earlier combined canonical ZA run passed in 1,428.06 seconds with all 183 forms,
the expected K deviation, G5 and cleanup. Its source predates these protocol-only
additions and remains recorded separately. The complete combined source must pass
hosted CI and the retention gate before PR84 merges into main.

PR85 was merged into release7-search at
`3be429208eb801b45fe08ec530b388375b6888c7`, after rechecking its expected head and
base. Incorporating that actual merge into the prepared branch made no file
changes. The retained source/binary receipts therefore still describe the tested
code. The final combined-head CI and retention counts remain pending; current
run links are recorded on PR84.
