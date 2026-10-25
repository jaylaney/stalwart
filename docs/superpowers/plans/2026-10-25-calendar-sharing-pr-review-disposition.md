# PR 58 review disposition

This is the historical disposition for the first 17 threads. The seven later threads and
their resulting contract corrections are recorded in the
[follow-up disposition](2026-10-25-calendar-sharing-followup-disposition.md).

Scope: all 17 inline threads fetched on 2026-10-25, reviewed against
`2eec6b61546a4c6dde17bcd819e0e153f1a9cc83`. The user authorized the primary to choose
worthwhile corrections. This records the selections, accepted limits and execution evidence below.
The [correction plan](2026-10-25-calendar-sharing-pr-review-corrections.md) owns execution.

| PR comment | Decision and reason |
| --- | --- |
| [4371584284](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584284) | Correct the vanished-path prefix to use the same path-segment encoder as actual DAV paths. Cover URI pchar and valid existing percent escapes, deletion delivery and private-sibling exclusion. |
| [4371584291](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584291) | Retain the original source snapshot/CAS after independent review. Refreshing only the delete archive can erase newer source metadata not copied to the destination, including after a stale conditional request. Preserve 409 and the newer source properties; committed child prefixes remain possible. An availability optimization would need a larger conditional/metadata-transfer design, so the original suggested one-line fix is declined. |
| [4371584292](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584292) | Skip precisely missing calendars in both shares-list loops. Other store/corruption failures remain errors; no broad omission. |
| [4371584294](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584294) | Restore error trace locations and gate unnecessary key-only roles/event preparation. Defer a wholesale plain/key scheduling-block split: the approved rewrite has subtle finalize/tag/error differences, and duplicating it would widen this correction. |
| [4371584299](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584299) | Defer memo-hit calendar allocation optimization. The mandatory operation-bit/own-id/personal-entry recheck remains; replacing it with a cached authorization result is unacceptable. A later archived-view optimization needs focused authorization coverage. |
| [4371584301](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584301) | Defer repeated collection-delete resolver optimization. The cost is real, but every physical event commit chunk must retain its calendar fence. A dedicated chunk-boundary regression should precede changing this storage path; moving one assertion before the loop is unsafe. |
| [4371584303](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584303) | Seed the assisted-discovery request memo from contexts freshly resolved in that request. Keep per-item access rechecks and prove resolver-read reuse without forbidden-path disclosure. |
| [4371584307](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584307) | Defer allocation and corrupt-state empty-home changes. Fresh store reads are intentional; a visible foreign home with no eligible children is already accepted. Do not substitute stale ACL cache data. |
| [4371584310](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584310) | Reuse the identical requested-calendar expression where this pass already changes propfind. Defer cross-file roles and scheduling-message helper extraction; it is refactoring beyond the concrete corrections. |
| [4371584314](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371584314) | Retain the approved session-cache policy. Keys are scoped by full ring identity/generation and die with SessionKeys under existing expiry/invalidation. Per-calendar pruning is a separate residency policy, not a correctness fix here. |
| [4371678740](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371678740) | Remove the pin assertion only from conversion guards. Keep vault/Account/calendar/event fences and the original final grant/re-key pin CAS. Prove a pin-only write does not cause a conversion assertion loss while a stale final pin decision is still rejected/redecided. A final decision loss can still cause a successful rewalk before publication; that optimization is not claimed. |
| [4371678745](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371678745) | Clarify the existing availability limit. Grant-ring AEAD failure can yield name:null; failure opening the caller's private key or an earlier vault/store failure can fail the list and assisted home at request level. Request-or-item 500 is already accepted. Defer partial-listing behavior until there is an explicit typed-cause/API decision; generic DataCorruption-to-null would hide other errors. |
| [4371678749](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371678749) | Record the specified 403/404 addressed-PROPFIND existence residual in the spec/API. A caller with some grant can confirm guessed paths, but cannot enumerate them through this mechanism. Preserve the specified 403 and the separate bulk tombstone exclusion. |
| [4371678753](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371678753) | Document that an error after commit does not mean the grant/revoke failed. Re-list/retry; retain pending-boundary protection and per-request work counts. No mutation-order redesign. |
| [4371678756](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371678756) | Document that reachable retries are sealed/key-only and cannot consume plain iTIP messages twice. No currently reachable dropped-message bug is established; defer a replay redesign until plain retries exist. |
| [4371678759](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371678759) | Document fail-closed ledger/ACL mismatch and qualified recovery. Preserve readable content; use a verified consistent backup or owner export/recreate/regrant where possible. No plaintext ACL-derived ledger repair and no blanket deletion remedy. |
| [4371679240](https://github.com/jaylaney/stalwart/pull/58#discussion_r4371679240) | Correct the overclaim in the barrier-only race comment, assert the ignored default-calendar DELETE result, and remove the tautological assertion. Keep the existing deterministic share-commit loss fixture as the actual retry proof. |

These decisions preserve revision 14's format, authorization, request/item corruption error
contract and accepted collection-operation prefixes. No claim of native, SQL, cluster,
historical-ciphertext erasure or hard socket-deadline qualification follows.

## Execution outcome

The selected corrections passed one complete independent Astra xhigh review and a scoped
correction re-review. Its two Important findings and one evidence-wording finding are addressed;
no new findings remain in the correction delta. The MOVE proposal was declined after meaningful
conditional and unconditional regressions demonstrated that it lost newer source properties.
The final code retains the original source-version fence.

| Validation | Observed result |
| --- | --- |
| Regression RED | URI tombstones, deleted-calendar listing, assisted double resolution and pin-only conversion loss reproduced; ordinary update trace lacked CausedBy. |
| MOVE property RED | Both unconditional and original-ETag If-Match MOVE returned 204 and deleted newer source property B; target A/one-child controls passed. |
| Core units | vault 45/groupware 146/common 121 passed. Vault/groupware sources stayed identical through the correction round; common was rerun after the forwarding change, 121 passed. |
| Standalone HTTP test_mode | The first required build exposed three E0433 feature-boundary errors; feature-safe forwarders corrected them, 6 unit tests passed. |
| Complete ZA suite | Final amended source: 1 suite passed, 0 failures, 353.98s, including conditional/unconditional source-property preservation and all existing modules. |
| WebDAV | Ordinary suite passed 16.88s; key-account suite passed 21.41s on the same frozen source. |
| Formatting/Python harness | Workspace formatting passed; all 30 harness tests passed. |
| Shipping RocksDB release build | Passed, 993.68s (16m33s); no enterprise/test_mode or release-profile override. Existing non-enterprise warnings retained. |
| Pinned production-client matrix | New shipping binary: ordinary 66 passed/1 skipped/5 xfailed (72), key 71 passed/1 skipped/5 xfailed (77); all five strict sharing flows passed. Whole runner 23.92s, exit 0. |

The compile/runtime freeze records 1746 tracked code/resource paths, including the new regression
module. The primary checks these hashes before and after each validation command; the final
source-freeze manifest SHA256 is
`d7746f14de26bf5d16875703dc0c52f43b39de5dc531a4be7f25d970e4c350f9`.
Source continuity also relies on explicit exclusive writer/build ownership; raw Cargo output does
not itself embed that manifest. Local commands, log hashes, source snapshots and review reports
are retained privately under this correction plan's ignored SDD workspace.

Earlier mutation and 3,000-event benchmark results remain evidence at their named source revisions
and were not rerun by this pass. Native Apple Calendar/iOS/Thunderbird, SQL and multi-node
qualification remain separate and pending.

CI at pushed head `2eec6b61` passed Style and the complete zero-access job, including key JMAP. Its
independent client job passed 30 harness tests and the ordinary 72-case matrix, then failed the
key-mode disposable server's 60-second readiness check before key provisioning/tests. Retained
sanitized logs do not identify whether recovery or normal startup timed out; no cause or proven
environment-flake classification is established. The earlier initial key-JMAP cleanup failure
also remains unexplained historical evidence. These are qualified separately from local results
and from CI on the eventual correction commit. No CI retry/deadline/policy relaxation was made.


The shipping artifact SHA256 is
`0e97cb56f986b74463aa75e7f1a4b215d71a2034d6c177cde57b03efc135cb5e`
(71,635,040 bytes). It was built and tested from the reviewed frozen source before the integration
commit. Toolchain: rustc 1.99.0/cargo 1.99.0; no release-profile environment overrides. The final
client run used isolated Python 3.12.15 and python-caldav commit
`111ae14b875f40d100d5259820f10210f00e20f9`; dependency pins and compatibility policy were unchanged.
The four retained client logs have 0700 directory/0600 file permissions and contain no fixture
password/token/header markers. Stores/config/users files were not retained; cleanup is also
covered by the harness failure-boundary tests.

Local qualification and independent review for the selected comment corrections are complete.
The existing PR remains draft; CI on the correction commit and the native gates remain separate.
