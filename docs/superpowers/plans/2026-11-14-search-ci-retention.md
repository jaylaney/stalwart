# Search CI retention repair

The owner authorized the PR84 correction and PR85 integration, with PR84 held
until the final combined CI is green. Hosted run 43509717996 now passes both ZA
storage modes with the unchanged five-second request deadline. Its remaining
failure is the Inventory gate: one deleted libtest and 24 function-step records.
Coverage gate and the affected runtime steps passed.

The retention review found exact moves and authorized expectation replacements,
plus still-valid metadata checks lost during the search test rewrite. C10 requires
their execution in the original steps. Release-7 section 9.3 replaces unconditional
keyed content refusal and singleton expectations; it does not retire metadata
filtering, sorting or pagination. Restore those checks before confirming mappings.

## Constraints

- Change tests and precise retention dispositions only. Preserve the baseline,
  gate algorithms, coverage threshold, selectors, retries and all existing cases.
- Keep current positive content/thread checks and keyless refusal checks.
- Adapt old metadata oracles only where mapped threading changes the expected
  grouping, thread predicates or ordered collapse representatives.
- Do not replace an independent oracle with the response under test, or claim
  that a green ZA suite supplies missing `imap-key`/`jmap-key` execution.
- Root owns Git and serializes builds/suites; source writers have separate files.
  Agents inherit the parent route because the capability ceiling is unresolved.

## Work

- [x] Restore IMAP metadata SORT SIZE against fetched RFC822.SIZE, the successful
  ARRIVAL COUNT/ALL case, and the flag-only SAVE/$ matrix including the size
  intersection. Retain the current upstream content checks in `imap-key`.
- [x] Restore the independent JMAP metadata corpus/readback oracle, 16 filter
  cases in both directions with/without collapse, size and thread-keyword sorts,
  and receivedAt paging/anchors in `jmap-key`. Preserve current ID completeness,
  content-currentness and upstream content-query checks.
- [x] Add exact Keyless function moves and source-backed changed-expectation
  dispositions for mapped singleton/thread behavior and served content. Record
  the deleted snippet-refusal internal subject and obsolete keyed-refusal
  assertions precisely; do not treat keyless refusal as equivalent.
- [x] Independently review the restorations and every retained-step mapping.
- [x] Run formatting, gate self-tests and complete key-mode JMAP/IMAP suites
  sequentially; retain exact source/binary/log evidence.
- [ ] Run the hosted retention net and strict-rule gate on the final combined
  source to prove the mappings.
- [ ] Integrate PR85 and the correction into release7-search, commit/push the
  reviewed combined changes, and keep PR84 unmerged until combined CI is green.

The existing combined local ZA result remains valid for its recorded source:
1,428.06 seconds, 183 forms, expected K deviation, G5 and cleanup. The additional
restorations passed both complete key-mode protocol suites; see the adjacent
retention outcome. No coverage-retirement waiver is taken.
