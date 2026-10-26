# Calendar sharing mutation execution evidence

This is the execution evidence for plan 14 Task 11, on commit `e58595e529f9c9e751e9534ebc507d62a8c27a24` plus the Task 11 detecting fixtures and the test-only oracles listed below. Structural scanning does not authenticate CKs, owner ledger ciphertext or associated data.

Baseline whole ZA passed with strict final empty cleanup (340.98s); fresh unit baseline common121, groupware146, vault45 passed. Each mutation was prepared as a unique context-anchored patch and inspected before execution, run alone with serialized Cargo, and restored by exact saved source bytes. No test failure is called a kill unless its named assertion/state or a causally established required positive acceptance path failed. Compile/setup/timeout/cleanup failures are inconclusive. No broken mutation is retained in the branch. Full patches below and before/mutated/restored hashes preserve auditability after ignored raw logs are removed. Commands select whole ZA; exact units are explicitly identified.

## Lossless patch rendering

Within diff-fenced hunks below, an empty displayed context line represents exactly one
ASCII space. Restore that single context-space prefix before applying a displayed patch or
checking its recorded SHA256. Every one of the85 durable diff fences has been rehydrated
under this convention and matches its recorded exact patch SHA256; raw patch receipts are
unchanged. This only removes trailing whitespace from Markdown rendering.

## Status

Fix round1 supplies valid required-case qualification for all57 rows. Scoped independent rereview approved all3 corrections with0 new Critical/Important/Minor findings; fresh restored wholeZA, affected units, formatting and whitespace checks passed (details below). Row19 attempt4 detects a forbidden group-only foreign href in case11 after both physical assisted eligibility checks are omitted under M6. Row48 attempt2 detects the missing second-only finalize in case10; its original attempt1 remains the case25 half. The former all57 claim was premature: attempt19/3 is invalid/superseded packaging qualification, and row48 had only partial coverage at that fixture version.

The85 executed attempts classify as69 valid kills (including row7's supplementary attempt2),11 inconclusive,4 survived (11/1,15/1,19/2,29/1), and1 invalid/superseded qualification (19/3). Every executed attempt retains exact raw receipts and restoration hashes. The invalid qualification observed a contract-permitted207 with a500 item and proves packaging only. Two further directories hold no execution evidence:5/1 prepared but never executed;6/1 interrupted during compilation and restored before a test ran. Historical gates apply to their named fixture hashes; fresh fix-round1 final gates passed as recorded below.

Fixture versions. `tests/src/za/leak.rs` is `f6c6fc69a9936d6ade4c897c615f3551a265da26396ad67312bafc5dc31d9d60` throughout. `tests/src/za/sharing.rs` was `4e64993b650f143be29725e89a264a9b2e3d886b4e1c4209c271a95dbc17d3b7` (the scanner baseline freeze) for every attempt started before oracle waves 1 and 2 were applied on 2026-10-24 (rows 1 to 36's attempts except 7/3, 11/2, 15/2, 19/3, 29/2 and 31/2); `f75c25fe377d7a95b94f172e052b2f1ebaeee0d12a4b5457d3818e55c740f4b7` after them (those six re-runs and rows 37 to 57; the unmutated whole ZA passed on it first, in 341.59s); and `6f56132a11dc151eee31ba9bccd3b1800a5d26c4d28bb7c95660b73ab8bb6f8e` after oracle46 (row46 attempt3 and historical final gates). Fix-round1 source `07053b2c5bfbb5c0cc5512bb71b46f349747385a9ff60c66f462c305134d8ab0` extracts one module-private envelope helper, accepts request/item500, adds the own-id eligibility oracle and makes case10 finalize observable. Its covering unmutated wholeZA passed1/0 in392.11s before19/4 and48/2. The `sharing.rs` line numbers in each attempt's section refer to the version that attempt ran against.

## Test-only oracles added during Task 11

Each is a patch to `tests/src/za/sharing.rs`, applied with `patch -p1 -F0` (no fuzz) in this order; none removes, weakens or reorders an existing assertion, adds a hook or changes cleanup. The original seven passed an unmutated whole-ZA run; oracle19's historical packaging failure is invalid qualification and is superseded in fix round1. Other historical mutation detections remain valid.

| Oracle | SHA256 | Row and ruling | Case | What it requires | Mutant attempt that fails at it |
|---|---|---|---|---|---|
| 07b | `8cce99f53f9aead8cc4313e272052dae86b50e2e7f5c56bb08b638482c1d7bd6` | 7, R11-7b | 21 | both held variants' stored X-ZA-KEY is a 0x03 envelope naming this calendar and a listed generation | 7/3 (creates only) |
| 11 | `91f927a132057817014b8cb188cf556ee9e1732e194d627fc11a9a9d933f0788` | 11, R11-11 | 6 | a reader revoke adds exactly one generation and keeps the earlier ones, the revoked reader's saved ring lacks it, a post-revoke owner PUT carries the newest | 11/2 |
| 15 | `70e6a55ddb2dde738d1687c632cd766cea2793661b07083558c0ae610de61f62` | 15, R11-15 | 11 | an edit grantee's PUT parked after its snapshot, then downgraded to view, answers 403 and leaves the archive byte-identical | 15/2 |
| 19 (historical, superseded) | `01e52266ec368c07a36de370c12a44a3c037a491f28ec333850febce57cebffe` | R11-19b rejected by R11-19-review | 11 | required whole-request500 instead of allowing request/item500 | 19/3 is invalid qualification |
| 29 | `a971a6f8207932fd4f69c880c920556795f1165c233f3fe9aeae2d16e7a0c115` | 29, R11-29 | 1 | an unlimited sync shows no `/private/` href and a limit-1 sync's token carries no continuation | 29/2 |
| 31 | `412847604df5e6f17aaf6a45e1ff6e9907dfcb1dd771b75f457c9ac4c32d353f` | 31, R11-31 | 1 | an unpaused rename into a former slug: the kept old-slug token answers 403 and a full sync shows no private tombstone | 31/2 |
| 46 | `334503021d034e4aee079b42561a6dd8cf1558d0be4db158f7956bda64346f00` | 46, R11-46(b) | 9 | the held external invitation's Schedule-Tag served on GET (to the owner and to the writer) is unchanged across the writer's attendee-only ACCEPTED and DELEGATED writes as the owner (RFC 6638 3.2.10) | 46/3 |

## Task 11 rulings

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

## Review disposition and remaining acceptance

- R11-19-review rejects historical R11-19b and invalidates attempt19/3. Attempt19/4 qualifies the fresh-eligibility rule under M6 through an actual forbidden href. Request500 or an exact corrupt-calendar item500 is accepted.
- Row48 now has both required halves: original global attempt1 at case25 and exact case10-scoped attempt2. The earlier insensitive fixture and missing attempt remain documented in the historical record.
- R11-validation-review accepts R11-25/R11-32 as logical faults removing every physical copy on the path, R11-13 as an M9 positive-grant preemption, and R11-22 scoped runs with their original preemptions preserved. These are qualified observations with the recorded limits, not broader claims.
- Duplicate `ck_envelope` logic is extracted to one module-private helper; existing assertions stay at their call sites. Scoped independent rereview approved all3 corrections with0 new findings (`task-11-fix-round1-rereview.md`); restored final gates passed as recorded below.
- Deferred Minor findings remain outside this fix wave: row35 changed-body coverage and inherited compiler/linker warnings. No native-client or Task12 acceptance is claimed.

## Summary of the 57 rows

"aN" is attempt N. `sharing.rs` line numbers are for the version the attempt ran against (see "Fixture versions").

| Row | Killing attempt(s) | Required case | Detection |
|---:|---|---|---|
| 1 | a1 | 16 | `sharing_races.rs:2631`: the parked grant's reader is still in the key cache |
| 2 | a1 | 17 (M2: revoke) | `sharing_races.rs:2511`: the revoked reader is still in the key cache |
| 3 | a1 | 17 | `sharing.rs:3408`: the re-keyed grantee is still in the key cache |
| 4 | a2 (endpoint-only; a1 inconclusive) | 18 | `utils/za.rs:145`: 200 not 409 after a vault change during the parked endpoint grant |
| 5 | a2 (endpoint-only) | 19 | `utils/za.rs:145`: 200 not 409 after a pin change during the parked endpoint grant |
| 6 | a2 (a1 interrupted, no evidence) | 20 | `sharing_races.rs:2537`: survivor bitmap 9 not 123 (revoke against level change) |
| 7 | a1; a2 supplementary (R11-7a); a3 creates-only (R11-7b) | 21 and 22 | a1 `sharing.rs:2348` rewrapped 1 not 0; a2 `sharing_races.rs:4191` held create under the pre-revoke generation; a3 oracle 07b `sharing.rs:2320`: the held write's event ends under envelope type Some(1) not Some(3) |
| 8 | a1 | 22 | `sharing_races.rs:4151`: the revoked grantee's DELETE 204 not 403 |
| 9 | a1 (M9) | 3 | `utils/za.rs:145`: 500 not 200 at the case-3 grant; Mutation9 skip line and `NotSealed` with ACL count 0 |
| 10 | a1 (M9, R11-10) | 3 | `utils/za.rs:145`: 500 not 200; one skip line and `NotSealed` |
| 11 | a2 (a1 survived) | 6 | oracle 11 `sharing.rs:3380`: "a revoke must add exactly one generation" |
| 12 | a1 | 8 | `sharing.rs:3322`: `Err(Aead)` under remaining-key substitution |
| 13 | a3 (scoped, M9; a1, a2 inconclusive) | 7 | 500 not 200 at case 7's grant, traced `Structure("grant header correspondence")`, nothing published (R11-13) |
| 14 | a1 | 11 | `sharing.rs:8348`: the group member's root PROPFIND lists the owner |
| 15 | a2 (a1 survived) | 11 | oracle 15 `sharing.rs:8593`: the parked PUT after a downgrade answers 204 not 403 |
| 16 | a1 | 11 | `utils/webdav.rs:599`: the grantee's depth-1 PROPFIND of the owner's home 403 not 207 |
| 17 | a1 | 11 | `sharing.rs:8250`: the gate-specific URI-gate check, 207 not 403 |
| 18 | a1 (Impersonate permission only) | 11 | `sharing.rs:8250`: 207 not 403 |
| 19 | a4 M6 (a1 inconclusive, a2 survived, a3 invalid/superseded) | 11 | `sharing.rs:8469`: Grace own-home propname emits forbidden Alice private href; request/item500 packaging accepted |
| 20 | a1 | 9 | `sharing.rs:5567`: the forged item is imported, copies 1 not 0 |
| 21 | a2 (Upstream provenance only; a1 inconclusive) | 9 | `sharing.rs:8709`: the organizer's REQUEST carries a stale SENT-BY |
| 22 | a2 (`case10-`; a1 detected at case 25) | 10 | `sharing.rs:9120`: the writer's own copy holds no token pairs |
| 23 | a2 (`case10-`; a1 detected at case 25) | 10 | `sharing.rs:9022`: the writer's own copy ACCEPTED not DECLINED |
| 24 | a2 (`case10-`; a1 detected at case 25) | 10 aliases | `sharing.rs:9121`: the per-alias token carrier is missing |
| 25 | a1 (every copy, R11-25) | 12 | `sharing.rs:747` via `utils/webdav.rs:599`: the linked MOVE 201 not 403 |
| 26 | a2 (`move-`; a1 inconclusive) | 12 | `sharing.rs:599`: the moved event's names 2 not 1 |
| 27 | a1, a2 | 14 race | `sharing_races.rs:3414`: 401 not 409; the parked grant committed after the owner's destruction |
| 28 | a1 | 5 alias transfer | `sharing.rs:3173`: 200 not 409 |
| 29 | a2 (a1 survived) | 1 | oracle 29 `sharing.rs:7144`: the limit-1 sync's token carries a continuation |
| 30 | a1 | 1 slug reuse | `sharing.rs:8527`: the old token 207 not 403 |
| 31 | a2 (a1 inconclusive) | 1 | oracle 31 via `utils/webdav.rs:599`: the unpaused rename's kept token 207 not 403 |
| 32 | a1 (every copy, R11-32) | 1 paused rename | `sharing.rs:7084`: the rename boundary does not follow the private history |
| 33 | a2 (`reused`; a1 inconclusive) | 1 paused rename | `sharing.rs:7068` via `utils/webdav.rs:599`: the initial sync (no token) during the pending boundary 403 not 207 |
| 34 | a1 | 24 first run | `sharing.rs:1438`: control #34, the earlier unlinked EWK candidate changed before the failed revert |
| 35 | a1 | 24 second run | `sharing.rs:2031` via `utils/webdav.rs:599`: the owner's PUT through the first name 204 not 409 |
| 36 | a1 (unit), a2 | 24 third run | a1 `vault` `session.rs:383`; a2 `sharing.rs:1701`: old cache identity reused |
| 37 | a1 (`case10-sole-decline`) | 10 | `sharing.rs:9523`: the writer's own copy NEEDS-ACTION not DECLINED |
| 38 | a1 (`case10-`) | 10 deletion | `sharing.rs:9546`: no STATUS:CANCELLED in the writer's own copy |
| 39 | a1, a2 (unit) | 25 | a1 `sharing.rs:7726` eleven step 0, owner run REQUEST, delegated none; a2 `second_run_uses_owner_locality` |
| 40 | a1 | 25 DTSTART edit | `sharing.rs:7726` eleven step 2: SEQUENCE 1 vs 2 |
| 41 | a1 | 25 DTSTART edit | `sharing.rs:7726` eleven step 2: SEQUENCE 1 vs 2 (double increment) |
| 42 | a1, a2 (unit) | 25 exception | a1 `sharing.rs:7834` exception-NEEDS-ACTION, the direct listed alias; a2 `snapshot_exception_leaves_scheduling_agent_and_payload_untouched` |
| 43 | a1 (`case10-sole-delegated`) | 10 | `sharing.rs:9546`: schedule tag unset, so no CANCEL |
| 44 | a1 | 25 | `sharing.rs:7726` eleven step 0: the delegated run delivers nothing |
| 45 | a1 | 25 | `sharing.rs:7907`: control #45, normalization must preserve an n+2 SEQUENCE difference |
| 46 | a1, a2 (unit), a3 (oracle 46) | 9 | a1 `sharing.rs:6378`, the direct gate assertion (R11-46 (a)); a2 `second_run_uses_owner_locality`; a3 oracle 46 `sharing.rs:9233`: after Bob's ACCEPTED write as Alice the served Schedule-Tag is `"2"` not `"1"` |
| 47 | a1, a2 (unit) | 25 CLIENT/NONE | a1 `sharing.rs:7726` agent-CLIENT step 0, owner none, delegated one REQUEST; a2 the CLIENT tree is not `NothingToSend` |
| 48 | a1 case25 + a2 exact case10 UID | 25 force-sole AND10 | historical `sharing.rs:7705` and fixed `sharing.rs:9575`: stored source keeps `SCHEDULE-FORCE-SEND` |
| 49 | a1 | 25 subaddress | `sharing.rs:7726` step 0: owner run 2 REQUEST items, delegated 1 |
| 50 | a1 | 25 subaddress statuses | `sharing.rs:7734` step 0: `bob+x` 1.2 vs 5.3 |
| 51 | a1 | Task 9 negative routing | `sharing.rs:4835`: the refused fan-out 1.2 not 5.3 |
| 52 | a1 | Task 9 catch-all trace | `sharing.rs:5511`: quiet classification leaked the catch-all canary |
| 53 | a1, a2 (unit) | 25 negative group | a1 `sharing.rs:7691` negative-group-DECLINED gets an attempted status; a2 the recipient list gains `custom@` and `writers@` |
| 54 | a1 | 25 sole-group | `sharing.rs:7668`: the group gets no status |
| 55 | a1 | 12 MOVE overwrite | `sharing.rs:834`: the MOVE overwrite builds a CANCEL, Some(true) not Some(false) |
| 56 | a1 | 25 failed direct alias | `sharing.rs:7680`: the alias's statuses [] not ["5.1"] |
| 57 | a1, a2 (unit) | 2 and 11.1 | a1 `sharing.rs:6965`: the owner's PROPFIND after the grantee's PROPPATCH lacks the owner's private name; a2 `shared_property_write_preserves_owner_parts` (`collection.rs:771`) |

## Final gates

Run serially on 2026-10-24 on the restored sources (`git diff --name-only -- crates` empty; `sharing.rs` `6f56132a11dc151eee31ba9bccd3b1800a5d26c4d28bb7c95660b73ab8bb6f8e`, `leak.rs` `f6c6fc69a9936d6ade4c897c615f3551a265da26396ad67312bafc5dc31d9d60`), each command prefixed with the rustup PATH export. No gate needed a rerun.

| Gate | Command | Result |
|---|---|---|
| Whole ZA | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture` | 1 passed, 0 failed, 361.98s (424s with the build); all 56 module markers, as in the unmutated run after oracle waves 1 and 2, through the strict final empty cleanup |
| Units | `cargo test -p vault -p groupware -p common` | common 121, groupware 146, vault 45 passed, 0 failed (equal to the baseline) |
| Product check | `cargo check -p stalwart --no-default-features --features rocks` | finished, exit 0 |
| Format | `cargo fmt --all --check` | clean, exit 0 |
| Whitespace | `git diff --check` | clean, exit 0 |

Every warning is inherited from unchanged files under `crates/` at HEAD (the production diff is empty): `store` 1 (unused import `LookupStore`) and `common` 18 (unused imports and variables, a field `data` that is never read) in the unit and check builds; `services` 2 (`task_manager/index.rs:24` and `:42`) and `jmap` 3 (`registry/mapping/principal.rs:24`, `registry/mapping/queued_message.rs:25` and `:43`), all unused imports, in the check build only, the one gate that builds those crates; and the za build's linker warning `__eh_frame section too large`. `cargo test -p http@0.16.26 --features test_mode` was not run: Task 11 changes only `tests/src/za/sharing.rs` and `tests/src/za/leak.rs`, which the http crate does not build.

## Attempts

| Row | Attempt | Disposition | Required case | Exit | Restoration |
|---:|---:|---|---|---:|---|
| 1 | 1 | killed | 16 (parked grant / precommit cache repopulation) | 101 | exact SHA256 match |
| 2 | 1 | killed | 17 (revoke cache eviction) | 101 | exact SHA256 match |
| 3 | 1 | killed | 17 (same-level rekey cache eviction) | 101 | exact SHA256 match |
| 4 | 1 | inconclusive for assigned case | 18 (vault change during parked endpoint grant) | 101 | exact SHA256 match |
| 4 | 2 | killed | 18 (vault change during parked endpoint grant) | 101 | exact SHA256 match |
| 5 | 2 | killed | 19 (pin change during parked endpoint grant) | 101 | exact SHA256 match |
| 6 | 2 | killed | 20 (grant racing a revoke on one calendar: both outcomes land) | 101 | exact SHA256 match |
| 7 | 1 | killed | 21 and 22 (PUT across a conversion; PUT across a revoke); detected at 21, update variant | 101 | exact SHA256 match |
| 8 | 1 | killed | 22 (DELETE across a revoke of the deleting grantee) | 101 | exact SHA256 match |
| 9 | 1 | killed | 3 (endpoint first grant converts the mixed sealed/plaintext `http-sharing` calendar; required 200 with rewrapped 1, sealed 1) | 101 | exact SHA256 match |
| 10 | 1 | killed | 3 (endpoint first grant converts the mixed sealed/plaintext `http-sharing` calendar; required 200 with rewrapped 1, sealed 1) | 101 | exact SHA256 match |
| 11 | 1 | survived | 6 (revoke: refused grantee, new generation, post-revoke event carries it, revoked saved ring lacks it) | 101 | exact SHA256 match |
| 12 | 1 | killed | 8 with remaining-key substitution (a revoke rotates while a remaining grantee's served key differs from its ledger key) | 101 | exact SHA256 match |
| 13 | 1 | inconclusive for assigned case | 7 (a grantee granted after rotations receives, and reads, every generation) | 101 | exact SHA256 match |
| 7 | 2 | killed (supplementary, ruling R11-7a) | 22 (PUT across a revoke; supplementary to the row-7 kill at 21) | 101 | exact SHA256 match |
| 13 | 2 | inconclusive for assigned case | 7 (a grantee granted after rotations receives, and reads, every generation) | 101 | exact SHA256 match |
| 14 | 1 | killed | 11 (resolver eligibility: a planted group grant does not admit its member) | 101 | exact SHA256 match |
| 15 | 1 | survived | 11 (resolver refuses a grantee whose own grant lacks the required bits) | 0 | exact SHA256 match |
| 16 | 1 | killed | 11 (a refused calendar is skipped in a listing: the home PROPFIND succeeds and lists only eligible calendars) | 101 | exact SHA256 match |
| 17 | 1 | killed | 11 (the gate refuses a Bearer token without its own session keys) | 101 | exact SHA256 match |
| 18 | 1 | killed | 11 (the gate refuses a grantee whose token carries the Impersonate permission) | 101 | exact SHA256 match |
| 19 | 1 | inconclusive for assigned case | 11 (with assisted discovery on, a grantee's own-home listing shows only eligible foreign calendars) | 101 | exact SHA256 match |
| 19 | 2 | survived | 11 (with assisted discovery on, a grantee's own-home listing shows only eligible foreign calendars) | 101 | exact SHA256 match |
| 13 | 3 | killed | 7 (a grantee granted after rotations receives, and reads, every generation) | 101 | exact SHA256 match |
| 20 | 1 | killed | 9 (the drain's delegated branch refuses a planted item whose SENT-BY does not resolve to its changed_by) | 101 | exact SHA256 match |
| 21 | 1 | inconclusive for assigned case | 9 (a delegated REQUEST carries its writer's SENT-BY, which carol's drain verifies against changed_by) | 101 | exact SHA256 match |
| 21 | 2 | killed | 9 (a delegated REQUEST carries its writer's SENT-BY, which carol's drain verifies against changed_by) | 101 | exact SHA256 match |
| 22 | 1 | inconclusive for assigned case | 10 (attendee token locality judged by the owner's addresses: Bob keeps his pair in Alice's event, so his later reply applies) | 101 | exact SHA256 match |
| 23 | 1 | inconclusive for assigned case | 10 (the writer's own inbox is admitted in a delegated fan-out: Bob's own copy advances, so his later reply is not stale) | 101 | exact SHA256 match |
| 24 | 1 | inconclusive for assigned case | 10 aliases (a delegated fan-out delivers one self item per writer alias, two attempts on one account) | 101 | exact SHA256 match |
| 22 | 2 | killed | 10 (attendee token locality judged by the owner's addresses: Bob keeps his pair in Alice's event, so his later reply applies) | 101 | exact SHA256 match |
| 23 | 2 | killed | 10 (the writer's own inbox is admitted in a delegated fan-out: Bob's own copy advances, so his later reply is not stale) | 101 | exact SHA256 match |
| 24 | 2 | killed | 10 aliases (a delegated fan-out delivers one self item per writer alias, two attempts on one account) | 101 | exact SHA256 match |
| 25 | 1 | killed | 12 (no single-parent rule: a MOVE of an event linked into two unshared calendars into a shared calendar is refused 403, its archive unchanged) | 101 | exact SHA256 match |
| 26 | 1 | inconclusive for assigned case | 12 (a successful unshared-to-shared MOVE leaves the event with the target as its sole parent, CK envelope, readable by owner and Bob) | 101 | exact SHA256 match |
| 27 | 1 | killed | 14 race (owner registry destruction during a parked authoritative grant: 409 account deleted, no orphan ACL index under a remaining grantee) | 101 | exact SHA256 match |
| 27 | 2 | killed | 14 race (owner registry destruction during a parked authoritative grant: 409 account deleted, no orphan ACL index under a remaining grantee) | 101 | exact SHA256 match |
| 28 | 1 | killed | 5 alias transfer (an address the ledger records for one grantee and the directory now resolves to another is ambiguous: 409, ring unchanged) | 101 | exact SHA256 match |
| 29 | 1 | survived | 1 (an incremental sync of a granted calendar emits its own tombstone and never counts or emits a tombstone of an ungranted calendar, result limit 1) | 101 | exact SHA256 match |
| 30 | 1 | killed | 1 slug reuse (a token taken before a private calendar at a reused slug was deleted answers 403 valid-sync-token once the new calendar is granted; the full sync shows nothing of the old one) | 101 | exact SHA256 match |
| 31 | 1 | inconclusive for assigned case | 1 rename into a former slug (a token kept from before a granted calendar was renamed into a slug that held a deleted private calendar answers 403 valid-sync-token; the full sync shows nothing of the old calendar) | 101 | exact SHA256 match |
| 32 | 1 | killed | 1 paused rename (private history committed while the rename is held before its commit stays behind the boundary: after completion the rename boundary exceeds the last private change id, and a numeric token above every pre-rename id is refused) | 101 | exact SHA256 match |
| 33 | 1 | inconclusive for assigned case | 1 paused rename (while the rename's boundary is pending, incremental syncs answer 403 valid-sync-token and the initial sync, no token, is served) | 101 | exact SHA256 match |
| 26 | 2 | killed | 12 (a successful unshared-to-shared MOVE leaves the event with the target as its sole parent, CK envelope, readable by owner and Bob) | 101 | exact SHA256 match |
| 33 | 2 | killed | 1 paused rename (while the rename's boundary is pending, incremental syncs answer 403 valid-sync-token and the initial sync, no token, is served) | 101 | exact SHA256 match |
| 34 | 1 | killed | 24 first run (phase 2's complete post-phase-1 parentage walk precedes every CK rewrap: an earlier sole-parent EWK row stays unchanged, with its account envelope, when a later row is found linked and the revert write fails) | 101 | exact SHA256 match |
| 35 | 1 | killed | 24 second run (a cut conversion journal with a raced-in EWK link refuses PUT and PROPPATCH through both names with 409 while the journal is shared, keeping both links and the stored data) | 101 | exact SHA256 match |
| 36 | 1 | killed | 24 third run (a session's collection-key cache keeps a key of an earlier conversion apart from the current one: the cache identity includes the key-domain id); attempt 1 runs the direct unit oracle vault::session::tests::collection_cache_separates_reconversions | 101 | exact SHA256 match |
| 36 | 2 | killed | 24 third run (a session's collection-key cache keeps a key of an earlier conversion apart from the current one: the cache identity includes the key-domain id); attempt 2 runs attempt 1's patch under the whole za suite | 101 | exact SHA256 match |
| 7 | 3 | killed | Case 21 (conversion contract, held-create variant), oracle R11-7b, row 7 narrowed to creates | 101 | exact SHA256 match |
| 11 | 2 | killed | Case 6 (reader revoke rotation), oracle R11-11 | 101 | exact SHA256 match |
| 15 | 2 | killed | Case 11 (grantee gate controls, downgrade after the request snapshot), oracle R11-15 | 101 | exact SHA256 match |
| 19 | 3 | invalid/superseded qualification | Case 11 (corrupt eligible calendar fails the assisted own-home listing), oracle R11-19 | 101 | exact SHA256 match |
| 29 | 2 | killed | Case 1 (limit-1 sync, private tombstone not counted), oracle R11-29 | 101 | exact SHA256 match |
| 31 | 2 | killed | Case 1 (rename into a former slug, unpaused; kept-token sync after the rename boundary), oracle R11-31 | 101 | exact SHA256 match |
| 37 | 1 | killed | 10 (self-delivery is not contingent on the first run: sole-attendee decline, fixture case10-sole-decline) | 101 | exact SHA256 match |
| 38 | 1 | killed | 10 (no self-CANCEL on a delegated deletion: Bob deletes the sole-attendee event, fixture case10-sole-decline) | 101 | exact SHA256 match |
| 39 | 1 | killed | 25 (second run judged by the merged list instead of owner locality: every step) | 101 | exact SHA256 match |
| 39 | 2 | killed | 25 (unit supplement: groupware calendar::delegation::tests::second_run_uses_owner_locality, same patch as attempt 1) | 101 | exact SHA256 match |
| 40 | 1 | killed | 25 (second run placed after the first run's finalize: the DTSTART edit) | 101 | exact SHA256 match |
| 41 | 1 | killed | 25 (upstream's public builder run on the real tree as the second run: the DTSTART edit, double SEQUENCE increment) | 101 | exact SHA256 match |
| 42 | 1 | killed | 25 (force_send set alone, RSVP kept: the exception's run, direct listed alias with RSVP=FALSE) | 101 | exact SHA256 match |
| 42 | 2 | killed | 25 (unit supplement: groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched, same patch as attempt 1) | 101 | exact SHA256 match |
| 43 | 1 | killed | 10 (schedule tag left unset when only the second run built: Bob creates the sole-attendee event, fixture case10-sole-delegated) | 101 | exact SHA256 match |
| 44 | 1 | killed | 25 (writer delivered from the first run's output instead of the second run's) | 101 | exact SHA256 match |
| 45 | 1 | killed | 25 (SEQUENCE normalized away by the differential helper; detected by baseline control #45 before case 25's steps) | 101 | exact SHA256 match |
| 46 | 1 | killed | 9 (the second run admitted on the attendee side; detection by the direct gate assertion in delegated_attendee_and_alias_edges, ruling R11-46 (a)) | 101 | exact SHA256 match |
| 46 | 2 | killed | 9 (unit supplement: groupware calendar::delegation::tests::second_run_uses_owner_locality, same patch as attempt 1) | 101 | exact SHA256 match |
| 47 | 1 | killed | 25 CLIENT/NONE (scheduling agent overridden; case 25 agent-CLIENT differential) | 101 | exact SHA256 match |
| 47 | 2 | killed | 25 CLIENT/NONE (unit supplement: groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched, same patch as attempt 1) | 101 | exact SHA256 match |
| 48 | 1 | killed | 25 sole ForceSend and 10 (reconciliation without finalize); killed at case 25 force-sole, the first of the row's two required cases | 101 | exact SHA256 match |
| 49 | 1 | killed | 25 sub-address item counts/tokens (case 25 subaddress label, step 0) | 101 | exact SHA256 match |
| 50 | 1 | killed | 25 owner's stored per-line SCHEDULE-STATUS (case 25 subaddress label, step 0) | 101 | exact SHA256 match |
| 51 | 1 | killed | Task 9 server-backed negative routing fixture (delegated_scheduling_tests, the `refused` fan-out of upstream-provenance messages with a request-local resolution) | 101 | exact SHA256 match |
| 52 | 1 | killed | Task 9 catch-all trace fixture (catch_all_resolution_is_quiet: R classification and delegated sender verification) | 101 | exact SHA256 match |
| 53 | 1 | killed | 25 group/unlisted/custom/catch-all negative exception (case 25 negative-group-DECLINED) | 101 | exact SHA256 match |
| 53 | 2 | killed | 25 group/unlisted/custom/catch-all negative exception (unit supplement: groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched, same patch as attempt 1) | 101 | exact SHA256 match |
| 54 | 1 | killed | 25 group status and eligible sole-group finalize (case 25 sole-group) | 101 | exact SHA256 match |
| 55 | 1 | killed | 12 actual outer delete-result Option seam in owner/Bob runs (copy_move_test: MOVE overwrite must not build a CANCEL) | 101 | exact SHA256 match |
| 56 | 1 | killed | 25 failed direct-alias exception: Run B 5.1 and no item; Run A suppressed (case 25 failed-direct-alias-DECLINED) | 101 | exact SHA256 match |
| 57 | 1 | killed | 2 and section 11.1 (case 2: a grantee's dead-property write must not overwrite the owner's personal bundle; grantee_dav_admission) | 101 | exact SHA256 match |
| 57 | 2 | killed | 2 and section 11.1 (unit supplement: groupware calendar::seal::collection::tests::shared_property_write_preserves_owner_parts, same patch as attempt 1) | 101 | exact SHA256 match |
| 46 | 3 | killed | 9 (the second run admitted on the attendee side; detection by oracle 46 in sharing_http_schedule_contracts, fixture case9-attendee-reply, ruling R11-46(b)) | 101 | exact SHA256 match |
| 19 | 4 | killed | 11 (M6 assisted own-home group-only foreign propname eligibility) | 101 | exact SHA256 match |
| 48 | 2 | killed | 10 sole delegated create (fixture-scoped; case25 separately qualified by attempt1) | 101 | exact SHA256 match |

## Mutation 1, attempt 1: killed

Required case: 16 (parked grant / precommit cache repopulation). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`. Patch SHA256: `94018dec52d7a2a3d3ee5d0801ef00ca9770e295e019780c8a31e5ea77d8e3de`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 115.955s. Decisive assertion: `sharing_races.rs:2631: !test.server.inner.cache.za_keys.contains_account(reader)`.

State evidence: Before release the reader own-home207 repopulated its cache and contains_account(reader) was true. The parked grant then returned200. The first postcommit assertion failed because the reader cache was still present; the later live-key-drop assertion was not reached. Whole ZA executed to this named HTTP sharing race and failed1/0passed in69.73s.

Ruling: Moving the sole explicit invalidation before the parked commit loses the postcommit eviction duty after a request repopulates the old cache. This is the prescribed ordering failure, not an HTTP500 or cleanup failure. Both production sources restored exactly before the next row.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/http/src/api/shares.rs` | `8ea709cb248858d020207a489f7608dc18e557b1e4b0eb6fbb7f6926e43f596f` | `5ee46a1cc6f65d777315b336877b97adba04e460ed0ceb54ee69cc91408b913e` | `8ea709cb248858d020207a489f7608dc18e557b1e4b0eb6fbb7f6926e43f596f` |
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `40db26be7851cfeac73e9f44e45283399cc1c35efec90ad8406e2aeefae2f315` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (reversed by restoring the saved exact bytes after execution):

```diff
--- a/crates/http/src/api/shares.rs
+++ b/crates/http/src/api/shares.rs
@@ -338,19 +338,14 @@
                 Err(error) => return share_error(error, verified.account_id, id),
             }
         }
         match za_share_apply(server, &verified.keys, id, change, &guard).await {
             Ok(applied) => {
                 // Acknowledged writes impose an immediate duty, before any fallible
                 // boundary completion can return. Equal ACL bits still invalidate.
-                let mut invalidation = CacheInvalidationBuilder::default();
-                for grantee in applied.affected_grantees {
-                    invalidation.invalidate(CacheInvalidation::AccessToken(grantee));
-                }
-                server.invalidate_caches(invalidation).await?;
                 if let Some(commit) = applied.boundary_commit {
                     za_complete_boundary(server, &verified.keys, id, Some(commit))
                         .await
                         .map_err(|e| e.into_store_error(verified.account_id, id))?;
                 }
                 trc::event!(
                     Store(trc::StoreEvent::DataWrite),
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -435,14 +435,19 @@
     calendar.update(
         changed_by,
         context.stored.to_unarchived::<Calendar>()?,
         keys.account_id,
         calendar_id,
         &mut batch,
     )?;
+    let mut invalidation = common::cache::invalidate::CacheInvalidationBuilder::default();
+    for grantee in &applied.affected_grantees {
+        invalidation.invalidate(common::ipc::CacheInvalidation::AccessToken(*grantee));
+    }
+    server.invalidate_caches(invalidation).await?;
     #[cfg(feature = "test_mode")]
     common::za::test_hooks::share_commit_pause_point(keys.account_id).await;
     let committed = server.commit_batch(batch).await?;
     server.notify_task_queue();
     if new_grant.is_some() {
         applied.boundary_commit = Some(ZaBoundaryCommit {
             change_id: committed.last_change_id(keys.account_id)?,
```

## Mutation 2, attempt 1: killed

Required case: 17 (revoke cache eviction). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`. Patch SHA256: `6837a41b4a7a0c74d56979e1bb9b6bea701246fac72afb7bfcddfc0d1281bf66`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 120.355s. Decisive assertion: `sharing_races.rs:2511: !test.server.inner.cache.za_keys.contains_account(reader)`.

State evidence: The real revoke was parked, reader own-home and foreign calendar requests returned207 before commit, and reader cache residency/live keys were sampled. The revoke returned200; the immediate explicit expected-eviction assertion failed because the reader cache remained present. Whole ZA failed in68.79s at the required HTTP race; prerequisite grants/lifecycle had completed.

Ruling: This patch omits AccessToken invalidation on revoke paths only, keeping grant setup functional so the required revoke case17 is exercised. It tests the missing physical invalidation duty in that real endpoint branch. No cache, resolver, crypto, receiver or cleanup rule was relaxed. Source restored exactly.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/http/src/api/shares.rs` | `8ea709cb248858d020207a489f7608dc18e557b1e4b0eb6fbb7f6926e43f596f` | `e3b9c0c1c85465af7ae800da51b9d954fc1599a36fec9ca5d5b06b8994d04a07` | `8ea709cb248858d020207a489f7608dc18e557b1e4b0eb6fbb7f6926e43f596f` |

Exact temporary patch (reversed by restoring the saved exact bytes after execution):

```diff
--- a/crates/http/src/api/shares.rs
+++ b/crates/http/src/api/shares.rs
@@ -340,15 +340,15 @@
         }
         match za_share_apply(server, &verified.keys, id, change, &guard).await {
             Ok(applied) => {
                 // Acknowledged writes impose an immediate duty, before any fallible
                 // boundary completion can return. Equal ACL bits still invalidate.
                 let mut invalidation = CacheInvalidationBuilder::default();
                 for grantee in applied.affected_grantees {
-                    invalidation.invalidate(CacheInvalidation::AccessToken(grantee));
+                    if revoke.is_none() { invalidation.invalidate(CacheInvalidation::AccessToken(grantee)); }
                 }
                 server.invalidate_caches(invalidation).await?;
                 if let Some(commit) = applied.boundary_commit {
                     za_complete_boundary(server, &verified.keys, id, Some(commit))
                         .await
                         .map_err(|e| e.into_store_error(verified.account_id, id))?;
                 }
```

## Mutation 3, attempt 1: killed

Required case: 17 (same-level rekey cache eviction). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `bc120b9795f97b2778c83af4b3bf085f1edecb4cfe12f252f95308f391ed031d`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 109.542s; test runtime 62.76s. Decisive assertion:
`sharing.rs:3408: !test.server.inner.cache.za_keys.contains_account(second_id)`.

State evidence: The fixture substituted the second grantee's key, created/read its own private
EWK event to ensure resident keys, pinned the replacement key, and issued a same-level view
grant. The endpoint returned200, then the immediate resident-cache eviction assertion failed.
The next live-key and ring-generation checks were not reached. This is the prescribed case17
cache duty after successful same-level rekey, not a generic HTTP or cleanup failure.

Ruling: For same-level rekeys only, the patch calls refresh_acls with identical old/new ACLs
and clears the explicit affected-grantee invalidation list. Grant setup and revoke invalidation
remain active. Refreshing unchanged ACL bits does not evict resident session keys. The restored
production diff is empty. The completed receipt was adjudicated after the worker interruption;
it was not rerun.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `408272cae3f29505937ea5527ae2e14f4ce71f484e55d1480afd4df5f6198d83` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -421,14 +421,17 @@
                 account_id: g.account_id,
                 grants: ZaShareLevel::from_code(g.level)?.bitmap(),
             })
         })
         .collect::<Result<Vec<_>, SealError>>()?;
     let entries = calendar.preferences.clone();
     za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
+    let mutation_same_level_rekey = applied.rotated && removed.is_empty() && new_grant.is_none()
+        && calendar.acls == context.stored.deserialize::<Calendar>()?.acls;
+    let mutation_acls = calendar.acls.clone();
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut batch = BatchBuilder::new();
     context.assert_in(&mut batch);
     guards.assert_in(&mut batch);
@@ -438,14 +441,18 @@
         keys.account_id,
         calendar_id,
         &mut batch,
     )?;
     #[cfg(feature = "test_mode")]
     common::za::test_hooks::share_commit_pause_point(keys.account_id).await;
     let committed = server.commit_batch(batch).await?;
+    if mutation_same_level_rekey {
+        server.refresh_acls(&mutation_acls, Some(&mutation_acls)).await?;
+        applied.affected_grantees.clear();
+    }
     server.notify_task_queue();
     if new_grant.is_some() {
         applied.boundary_commit = Some(ZaBoundaryCommit {
             change_id: committed.last_change_id(keys.account_id)?,
         });
     }
     applied.affected_grantees.sort_unstable();
```

## Mutation 4, attempt 1: inconclusive for assigned case

Required case: 18 (vault change during parked endpoint grant). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `66015fa781e068431bbaa9cadb7c2c26d41dac94e1282770ff40c1eede47fed7`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 197.671s. Decisive observation:
`sharing.rs:1310: expected opaque conversion conflict, got Ok(ZaConversionCounts { rewrapped: 0, sealed: 0 })`. Whole ZA failed in64.15s.

State evidence: The globally omitted vault assertion was observed earlier by direct conversion guard fixtures; the required HTTP password-change race was not reached.

Ruling: This establishes sensitivity of the conversion guard oracle but does not qualify row4 case18. A separate endpoint-only omission will preserve prerequisite guards and execute that assignment. Exact source restoration verified before attempt2.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `cd0d24e0afd56cf71f77758202d0fcc12045cc8bd43cc3b28c72173dcc70ecb3` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -108,19 +108,15 @@

 impl ZaShareGuards {
     /// Append every externally verified fence before any indexed write.
     pub fn assert_in(&self, batch: &mut BatchBuilder) {
         batch
             .with_account_id(self.owner_id)
             .with_collection(Collection::Principal)
-            .with_document(0)
-            .assert_value(
-                PrincipalField::ZeroAccessVault,
-                AssertValue::Hash(self.vault_cas),
-            );
+            .with_document(0);
         if let Some(pin) = self.pin_assertion {
             batch.assert_value(PrincipalField::ZeroAccessPins, pin);
         }
         batch.assert_value(
             ValueClass::Registry(RegistryClass::Item {
                 object_id: ObjectType::Account.to_id(),
                 item_id: Id::from(self.owner_id).id(),
```

## Mutation 4, attempt 2: killed

Required case: 18 (vault change during parked endpoint grant). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `a9c56a312ac9f2d72d950279d8ff862000ee5d47e010cb177ad29a62341c3887`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 119.065s. Decisive observation:
`utils/za.rs:145`: expected409, received200 with `{"ok":true,"changed":true,"rewrapped":0,"sealed":0}`; whole ZA runtime67.43s.

State evidence: Direct converter/rollback guards and the earlier pin-forget/pin-missing races completed with original guards. In sharing_http_races, the grant was parked with verified old request keys, a real owner password change returned200, and releasing the grant returned200 rather than required409/vault changed since verification. The later unchanged-calendar-version assertion was not reached.

Ruling: The temporary helper duplicates the original guard assembly except its vault assertion, and is called only by za_share_apply. This exposes the assigned physical endpoint fence while preserving prerequisite conversion guards, pin and Account assertions, indexed calendar fences, and all test assertions. Both helper and call restored exactly; no permanent API change.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `af9952a630b5663ca98ee6e1e171ec2fb4c683591c685e0f43702a1894778b96` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `e04e057dff18a2e40f2f0c6189b49610c840f9d42e4a5f5558eec0a64961ec50` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -104,14 +104,30 @@
             ZaCalendarError::Store(error) => Self::Store(error),
         }
     }
 }

 impl ZaShareGuards {
     /// Append every externally verified fence before any indexed write.
+    pub fn assert_in_without_vault(&self, batch: &mut BatchBuilder) {
+        batch
+            .with_account_id(self.owner_id)
+            .with_collection(Collection::Principal)
+            .with_document(0);
+        if let Some(pin) = self.pin_assertion {
+            batch.assert_value(PrincipalField::ZeroAccessPins, pin);
+        }
+        batch.assert_value(
+            ValueClass::Registry(RegistryClass::Item {
+                object_id: ObjectType::Account.to_id(),
+                item_id: Id::from(self.owner_id).id(),
+            }),
+            AssertValue::Some,
+        );
+    }
     pub fn assert_in(&self, batch: &mut BatchBuilder) {
         batch
             .with_account_id(self.owner_id)
             .with_collection(Collection::Principal)
             .with_document(0)
             .assert_value(
                 PrincipalField::ZeroAccessVault,
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -427,15 +427,15 @@
     za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut batch = BatchBuilder::new();
     context.assert_in(&mut batch);
-    guards.assert_in(&mut batch);
+    guards.assert_in_without_vault(&mut batch);
     calendar.update(
         changed_by,
         context.stored.to_unarchived::<Calendar>()?,
         keys.account_id,
         calendar_id,
         &mut batch,
     )?;
```

## Mutation 5, attempt 2: killed

Required case: 19 (pin change during parked endpoint grant). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `11a9791cc05bb425bab2f47b27d60b2e79ec11bdc8f2bfda099ad794cd632a61`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 113.959s. Decisive observation:
`utils/za.rs:145`: expected409, received200 with `{"ok":true,"changed":true,"rewrapped":0,"sealed":0}`; whole ZA runtime67.34s.

State evidence: The real grant in sharing_http_races was parked before its endpoint commit; the owner forgot the grantee pin and the forget returned200. Releasing the held grant returned200 instead of the required409/contact not pinned. Prerequisite direct conversion guards, concurrent grants and revoke/cache checks completed with original guards. The subsequent missing-pin/password races were not reached.

Ruling: The temporary helper duplicates the original guard assembly except its optional pin assertion, and only za_share_apply calls it. Vault, Account, calendar and event guards and every test assertion remain active. This scopes the physical endpoint pin fence to the assigned case19, preserving earlier converter guards. Both temporary helper and call restored exactly. Attempt1 was prepared before this scope refinement but never executed.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `83f08fb6afc478adef06c6a25a18a2da4560162ad993225f6f13adff084e264f` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `4ed59f4c70b5702c63b9a6d8feb9bfc68c7960287ed850411e3f7465cf12538c` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -104,14 +104,31 @@
             ZaCalendarError::Store(error) => Self::Store(error),
         }
     }
 }

 impl ZaShareGuards {
     /// Append every externally verified fence before any indexed write.
+    pub fn assert_in_without_pin(&self, batch: &mut BatchBuilder) {
+        batch
+            .with_account_id(self.owner_id)
+            .with_collection(Collection::Principal)
+            .with_document(0)
+            .assert_value(
+                PrincipalField::ZeroAccessVault,
+                AssertValue::Hash(self.vault_cas),
+            );
+        batch.assert_value(
+            ValueClass::Registry(RegistryClass::Item {
+                object_id: ObjectType::Account.to_id(),
+                item_id: Id::from(self.owner_id).id(),
+            }),
+            AssertValue::Some,
+        );
+    }
     pub fn assert_in(&self, batch: &mut BatchBuilder) {
         batch
             .with_account_id(self.owner_id)
             .with_collection(Collection::Principal)
             .with_document(0)
             .assert_value(
                 PrincipalField::ZeroAccessVault,
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -427,15 +427,15 @@
     za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut batch = BatchBuilder::new();
     context.assert_in(&mut batch);
-    guards.assert_in(&mut batch);
+    guards.assert_in_without_pin(&mut batch);
     calendar.update(
         changed_by,
         context.stored.to_unarchived::<Calendar>()?,
         keys.account_id,
         calendar_id,
         &mut batch,
     )?;
```

## Mutation 6, attempt 2: killed

Required case: 20 (grant racing a revoke on one calendar: both outcomes land). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `7d7d6006e94b7c84c0cbe58c2e66d3de8d883bb95b2d3eb1a0a91f42898c8eaf`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 231.131s. Decisive observation:
`sharing_races.rs:2537`: `assert_eq!(survived.acls[0].grants.bitmap, 123)` failed with left `9`, right `123`. Whole ZA failed 0 passed/1 failed in 67.74s (build 2m36s).

State evidence: The run log shows the helper's line `Mutation6: removed both matching calendar archive fences; retained vault/pin/Account and all writes` for every za_share_apply commit, so its exact-count check (2 removed per batch) held each time; earlier sharing fixtures that call the endpoint passed under it. In sharing_http_races the two barrier-released concurrent grants (za_post_concurrent, no pause hook) both answered 200 and the calendar held ACL 2 / preferences 3, so that half did not show a lost update. The two requests are not parked, so their overlap at the share commit is left to timing; an inference from the code, not observed in the log: both first run the conversion, whose fenced Init write lets one win, and the loser restarts its decision loop (guards, calendar lookup, pin read, precheck, conversion) before it reads the calendar for its share commit, which makes it likely to read after the winner's commit. The deterministic half then ran: the revoke of the reader was parked at SHARE_COMMIT_PAUSE with its batch already assembled and unfenced, the level change of the second grantee to edit committed 200 in between, and the released revoke answered 200 and committed its stale calendar. The reader's 403, the second grantee's 207 and the cache-eviction assertions held; the survivor had one ACL entry for the second grantee, but with view bits 9 (Read|ReadItems) instead of edit 123: the level change was overwritten (lost update).

Ruling: Killed at case 20's grant-versus-revoke race. With both physical calendar ARCHIVE assertions removed from the endpoint batch (the explicit context.assert_in and the ObjectIndexBuilder update fence), a revoke that read the calendar before a concurrent level change commits over it, and the level change's outcome does not land, which is what case 20 requires. The patch is ruling M6's temporary BatchBuilder-internal filter, matching only owner/Calendar/calendar id/ARCHIVE AssertValue ops and called only from za_share_apply after batch assembly; vault, pin and registry Account assertions and every write, index and log op are retained. Attempt 1 used the identical patch (same SHA256) but was interrupted by the user during compilation before any test ran and was restored from its saved bytes by the coordinator; it produced no evidence and is not recorded as a result. Both files restored to their exact pre-mutation bytes; production diff empty afterwards.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/store/src/write/batch.rs` | `042bb6581362e84fa75a2e5ae17550ac2d74c0644a867bdb5d07d459e9927e2a` | `58cbfb625623b20987ecda740c2d245c085f9e7d3ae9c22479605c96e6d54fbc` | `042bb6581362e84fa75a2e5ae17550ac2d74c0644a867bdb5d07d459e9927e2a` |
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `1c1cda3905f59ae20e7529373c8215733937069706bfc8f9863ab2798f430bda` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/store/src/write/batch.rs
+++ b/crates/store/src/write/batch.rs
@@ -457,14 +457,39 @@
         }
     }

     pub fn has_changes(&self) -> bool {
         !self.changed_collections.is_empty()
     }

+    // Temporary mutation6 only: remove both physical target-calendar fences.
+    pub fn za_mutation_unfence_calendar(&mut self, owner: u32, calendar: u32) {
+        assert!(self.commit_points.is_empty(), "mutation6 requires one assembled endpoint batch");
+        let (mut account, mut collection, mut document) = (None, None, None);
+        let mut removed = 0;
+        self.ops.retain(|op| {
+            match op {
+                Operation::AccountId { account_id } => account = Some(*account_id),
+                Operation::Collection { collection: value } => collection = Some(*value),
+                Operation::DocumentId { document_id } => document = Some(*document_id),
+                Operation::AssertValue { class: ValueClass::Property(field), .. }
+                    if account == Some(owner) && collection == Some(Collection::Calendar)
+                        && document == Some(calendar) && *field == u8::from(types::field::Field::ARCHIVE) => {
+                    removed += 1; return false;
+                }
+                _ => (),
+            }
+            true
+        });
+        assert_eq!(removed, 2, "mutation6 must remove explicit and indexed-writer fences only");
+        self.batch_ops -= removed;
+        self.has_assertions = self.ops.iter().any(|op| matches!(op, Operation::AssertValue { .. }));
+        eprintln!("Mutation6: removed both matching calendar archive fences; retained vault/pin/Account and all writes");
+    }
+
     pub fn ops(&self) -> &[Operation] {
         self.ops.as_slice()
     }

     pub fn len(&self) -> usize {
         self.batch_size
     }
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -435,14 +435,15 @@
     calendar.update(
         changed_by,
         context.stored.to_unarchived::<Calendar>()?,
         keys.account_id,
         calendar_id,
         &mut batch,
     )?;
+    batch.za_mutation_unfence_calendar(keys.account_id, calendar_id);
     #[cfg(feature = "test_mode")]
     common::za::test_hooks::share_commit_pause_point(keys.account_id).await;
     let committed = server.commit_batch(batch).await?;
     server.notify_task_queue();
     if new_grant.is_some() {
         applied.boundary_commit = Some(ZaBoundaryCommit {
             change_id: committed.last_change_id(keys.account_id)?,
```

## Mutation 7, attempt 1: killed

Required case: 21 and 22 (PUT across a conversion; PUT across a revoke); detected at 21, update variant. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `f5645ed2eea9bafae80c95340c779f2fa11c4b11c6204022165313e378a3a04c`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 136.063s. Decisive observation:
`sharing.rs:2348`: `assert_eq!(converter.finish().await.unwrap(), ZaConversionCounts::default())` failed with left `ZaConversionCounts { rewrapped: 1, sealed: 0 }`, right `ZaConversionCounts { rewrapped: 0, sealed: 0 }`. Whole ZA failed 0 passed/1 failed in 66.75s (build 1m02s; the only new warning is the expected `unused variable: contexts` in the create closure at update.rs:789).

State evidence: In conversion_contract_tests (sharing.rs), the T6-U fixture `held-calendar-only`: an owner PUT update of a planted EWK event was parked at UPDATE_COMMIT_PAUSE after reading the unshared calendar, a direct conversion was then parked at SHARE_REWRAP_PAUSE after its phase-1 Init write, and the held PUT was released and answered 204 (line 2347 held). The converter, resumed, found the event still EWK and rewrapped it (rewrapped 1); the original code makes the held PUT fail its calendar assertion, retry under the fresh phase-1 ring and seal under the CK, so the converter skips it (count 0). Before it, the held-create and held-update loop at sharing.rs:2203 passed under the mutation: the update variant answers 409 either way because the conversion rewrapped the held event and the event's own archive assertion catches it, and the create variant's checks only open the event in the owner's shared domain, which by design also opens EWK (0x01) rows (classify_domain_read's owner_legacy path), so a stale EWK create is not distinguished there. Case 22 (sharing_races grantee_http_write_races and the owner update across rotation) was not reached.

Ruling: Killed at case 21 (a PUT that read the calendar before phase 1, held by UPDATE_COMMIT_PAUSE until after it). With the current-calendar assertions removed from both DAV PUT commit seams (update at update.rs:481 and create at update.rs:797; the event's own archive assertion and the UID lease assertion retained), the held update commits under the calendar's pre-conversion key domain instead of retrying under the current ring, which the exact conversion count exposes. Case 22 was not reached; no extra attempt was run for it, as instructed. Finding for the coordinator: the case-21 create variant (held-create at sharing.rs:2203) has no oracle for the stored envelope type, so it would not detect this condition alone. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `4e6b971ebd00518dc5e80a54ce2c01db651ac1e35904e1b2bcd6bbc49b12d45a` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -475,17 +475,14 @@

             // The same event/index/alarm intent is rebuilt once, without scheduling.
             let schedule_tag = new_event.schedule_tag;
             let timestamp = store::write::now() as i64;
             let mut etag;
             for attempt in 0..=1 {
                 let mut batch = BatchBuilder::new();
-                for context in &contexts {
-                    context.assert_in(&mut batch);
-                }
                 etag = new_event
                     .clone()
                     .update_at(
                         access_token.account_tenant_ids(),
                         event_.to_unarchived::<CalendarEvent>()?,
                         account_id,
                         document_id,
@@ -791,17 +788,14 @@
                 |event: &CalendarEvent,
                  contexts: &[groupware::calendar::sharing::resolve::ZaCalendarContext]|
                  -> trc::Result<(BatchBuilder, Option<String>)> {
                     let mut batch = BatchBuilder::new();
                     if let Some((lease, _)) = &za_lease {
                         lease.lease().assert_in(&mut batch);
                     }
-                    for context in contexts {
-                        context.assert_in(&mut batch);
-                    }
                     let etag = event
                         .clone()
                         .insert_at(
                             access_token.account_tenant_ids(),
                             account_id,
                             document_id,
                             next_email_alarm.clone(),
```

## Mutation 8, attempt 1: killed

Required case: 22 (DELETE across a revoke of the deleting grantee). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `c2cf1d770e3016203d47b4385404aac42733d64dee87f10f612ce43ec39c02c1`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 106.297s. Decisive observation:
`sharing_races.rs:4151`: `assert_eq!(response.status, StatusCode::FORBIDDEN, "revoked Bob {operation}: {}")` failed with message `revoked Bob DELETE: ` (empty body), left `204`, right `403`. Whole ZA failed 0 passed/1 failed in 68.35s (build 30.94s).

State evidence: In sharing_races grantee_http_write_races (the first fixture of sharing_races::test), the three rotated variants (Carol revoked while Bob's CREATE, UPDATE and DELETE were held) passed, the rotated DELETE committing on its first attempt instead of its calendar-conflict retry with the same 204. In the revoked pass, Bob's CREATE and UPDATE held across his own revoke still answered 403 (their PUT seams are unmutated), the revoke answered 200 and advanced the ring generation (line 4148 held), and Bob's DELETE, parked at DELETE_COMMIT_PAUSE after its fan-out, was released and answered 204: the deletion committed for a grantee whose grant had been removed. The event-unchanged assertion that follows was not reached; the 204 is the store's acknowledgement of the DestroyArchive batch.

Ruling: Killed at case 22. With both calendar context assertions removed from the event DELETE (the first commit at delete.rs:303 and its calendar-conflict retry at delete.rs:398), a DELETE that read the calendar while Bob held a grant commits after the revoke; the original fails the calendar assertion, re-reads the context, finds Bob forbidden and answers 403 with the event kept. The event's own archive assertion and everything else in the batch are unchanged. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/delete.rs` | `1e94fadafe67490990e2714725ac4f2ac3da5b089a793b3901df8568a6cad66b` | `aa0faec326f0be537431408a2c3abe08b93d4f5006456606c2ba5c0ecea6db60` | `1e94fadafe67490990e2714725ac4f2ac3da5b089a793b3901df8568a6cad66b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/delete.rs
+++ b/crates/dav/src/calendar/delete.rs
@@ -296,15 +296,14 @@
                 account_id,
                 calendar_id,
                 utils::map::bitmap::Bitmap::new().with_item(Acl::RemoveItems),
             )
             .await
             .map_err(|error| za_calendar_error(error, account_id, calendar_id))?
             {
-                context.assert_in(&mut batch);
                 contexts.push(context);
             }
             let cutoff = store::write::now() as i64;
             retry_event = Some((event_.clone(), calendar_id, cutoff));
             // Delete event
             za_messages.extend(
                 DestroyArchive(event)
@@ -390,17 +389,14 @@
                 .map_err(|error| za_calendar_error(error, account_id, *calendar_id))?
                 .into_iter()
                 .collect::<Vec<_>>();
                 if !calendars_changed(&contexts, &fresh) {
                     return Err(error.into());
                 }
                 let mut batch = BatchBuilder::new();
-                for context in &fresh {
-                    context.assert_in(&mut batch);
-                }
                 DestroyArchive(original.to_unarchived::<CalendarEvent>()?).delete_za_at(
                     &account_info,
                     account_id,
                     document_id,
                     *calendar_id,
                     resources.format_resource(delete_resource).into(),
                     false,
```

## Mutation 9, attempt 1: killed

Required case: 3 (endpoint first grant converts the mixed sealed/plaintext `http-sharing` calendar; required 200 with rewrapped 1, sealed 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `1b7413f4b89c515de5857b695a4cce702283d90e427946d1a3ecd5a9915859da`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 124.961s. Decisive observation:
`utils/za.rs:145` (VaultReply::expect, called by the case-3 grant at sharing.rs:2754): `unexpected status`, left `500`, right `200`, body `{"type":"about:blank","status":500,"title":"Internal Server Error",...}`. Whole ZA failed 0 passed/1 failed in 63.67s (build 54.29s).

State evidence: Preconditions checked before preparation: the case-3 fixture creates the calendar with MKCALENDAR at `.../http-sharing/` and its first grant answers 409 `contact not pinned`, which the endpoint can only reach after `calendar_id` matched the stored `Calendar.name` against `http-sharing`; it plants one release-1 sealed event (`plant_sealed_event` -> `seal_event` in the owner's unshared domain, wrap type 0x01) and one plaintext event before that grant; every type, variant and import the patch uses exists in convert.rs (`ZaShareError::Seal`, `SealError::NotSealed`, `Calendar`, `Archive`, `AlignedBytes`, `ValueKey`, `Collection`, `Server`). The run log shows, in order, `Mutation9 skipped acknowledged EWK-to-CK rewrite on case-3 calendar: event=1` (the one Sealed-kind row, skipped before rewrap_event and write_event, so it stays EWK) and `Mutation9 final forward scan returned NotSealed; calendar=http-sharing, ACL count=0, no grant published`, then the panic. The 409 pre-pin grant and the pin call passed (the skip line is printed only from the grant's conversion). The final forward scan returns NotSealed only for a non-CK row, the plaintext row having been sealed, so the remaining non-CK row is the skipped EWK event; the conversion returned the error before za_share_apply, and the calendar's ACL list was empty, so no grant was published. share_error maps Seal(NotSealed) to DataCorruption, hence the 500.

Ruling: Killed at case 3 under ruling M9: the required positive acceptance path (grant 200 with rewrapped 1) failed causally, with the exact origin recorded (the conversion's final forward validate_scan, NotSealed), the stored state (the sealed event left EWK, ACL count 0, no grant published) and the mutation's own skip line. The omission is fixture-scoped as M9 approves (only Sealed-kind rows of a calendar whose stored name is `http-sharing`), leaving the guards, the final scan, the counters, every assertion and all other fixtures unchanged; the NotSealed line is an observation-only trace on the error path that returns the same error. The patch is the coordinator-reviewed draft, unchanged. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `9ba19a5b6622c23d4ed3544f3432c9fe5820d55f746c496fede8498551f6e655` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -259,14 +259,20 @@
                             return Err(SealError::Structure("collection key event parent").into());
                         }
                         let kind = classify_event_storage(&event)?;
                         if is_ck(&event, kind)? {
                             unseal_event_in_domain(&mut event, &context.domain)?;
                             continue;
                         }
+                        if kind == ZaEventStorageKind::Sealed
+                            && context.stored.deserialize::<Calendar>().is_ok_and(|c| c.name == "http-sharing")
+                        {
+                            eprintln!("Mutation9 skipped acknowledged EWK-to-CK rewrite on case-3 calendar: event={}", id);
+                            continue;
+                        }
                         rewrap_event(&mut event, &context.domain, &context.domain)?;
                         write_event(
                             server,
                             guards,
                             changed_by,
                             &context,
                             *id,
@@ -281,15 +287,20 @@
                             ZaEventStorageKind::LegacyPlaintext => counts.sealed += 1,
                             ZaEventStorageKind::Sealed => counts.rewrapped += 1,
                         }
                     }
                 }
                 // No grant-ready result until every current row has sole parent
                 // and an authenticated envelope in this ring's domain.
-                validate_scan(server, &context, true).await?;
+                let checked = validate_scan(server, &context, true).await;
+                if matches!(&checked, Err(ZaShareError::Seal(SealError::NotSealed))) {
+                    let current = read_calendar_for_mutation9(server, keys.account_id, calendar_id).await?;
+                    eprintln!("Mutation9 final forward scan returned NotSealed; calendar={}, ACL count={}, no grant published", current.name, current.acls.len());
+                }
+                checked?;
                 return Ok(counts);
             }
         }
     }
 }

 pub(super) fn owner_ring(
@@ -514,14 +525,19 @@
         .await?;
     Ok((ids, linked))
 }

 /// At most one logical group's snapshots survive across writes/a pause. Reading
 /// the next group before the between-group seam also makes genuine archive CAS
 /// races observable without holding the entire calendar in memory.
+async fn read_calendar_for_mutation9(server: &Server, owner: u32, calendar: u32) -> Result<Calendar, ZaShareError> {
+    let stored: Archive<AlignedBytes> = server.store().get_value(ValueKey::archive(owner, Collection::Calendar, calendar)).await?.ok_or(SealError::NotSealed)?;
+    Ok(stored.deserialize::<Calendar>()?)
+}
+
 async fn read_chunk(
     server: &Server,
     owner: u32,
     ids: &[u32],
 ) -> Result<Vec<(u32, Archive<AlignedBytes>)>, ZaShareError> {
     let mut chunk = Vec::with_capacity(ids.len());
     for &id in ids {
```

## Mutation 10, attempt 1: killed

Required case: 3 (endpoint first grant converts the mixed sealed/plaintext `http-sharing` calendar; required 200 with rewrapped 1, sealed 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `f293ee1f0a24ea5d80a228574db13bffa2d1a5ccd15a3c8e03dd106a5e256045`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 126.511s. Decisive observation:
`utils/za.rs:145` (VaultReply::expect, called by the case-3 grant at sharing.rs:2754): `unexpected status`, left `500`, right `200`, body `{"type":"about:blank","status":500,"title":"Internal Server Error",...}`. Whole ZA failed 0 passed/1 failed in 62.05s (build 56.96s).

State evidence: The run log shows `Mutation10 skipped legacy plaintext event in conversion: event=2` (the planted plaintext row, skipped before rewrap_event and write_event, so it stays plaintext; row 9's run identifies event 1 as the sealed row of the same fixture) and `Mutation10 final forward scan returned NotSealed; calendar=http-sharing, ACL count=0`, then the panic. No Mutation10 line appears earlier in the run, so the global skip met no earlier conversion holding a plaintext row; that matches the source, where the first `plant_event` into a calendar that is later converted is this fixture's. The 409 pre-pin grant and the pin call passed. The final forward scan returns NotSealed for a non-CK row; the sealed row was rewrapped, so the remaining non-CK row is the skipped plaintext event; the conversion returned the error before za_share_apply, with an empty ACL list, so no grant was published. share_error maps Seal(NotSealed) to DataCorruption, hence the 500. Case 3's second run (the untouched default calendar with a planted plaintext event) was not reached.

Ruling: Killed at case 3 on the same evidentiary standard as ruling M9: the required positive acceptance path (grant 200 with sealed 1) failed causally, with the exact origin (the conversion's final forward validate_scan, NotSealed), the stored state (the plaintext event left unsealed, ACL count 0, no grant published) and the mutation's own skip line recorded. The skip is global, as the ledger states the condition; no scoping was needed because no earlier fixture converts a calendar holding a plaintext row. The draft was extended, before execution, with observation-only traces in the style of row 9's coordinator-approved patch (a skip line and a NotSealed line on the error path that returns the same error); the condition's code change itself is the draft's one-line skip. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `3dd158c15f3bf76df7b74906533068f3be97a43c2f80e79b8fab0d70fad93f17` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -259,14 +259,18 @@
                             return Err(SealError::Structure("collection key event parent").into());
                         }
                         let kind = classify_event_storage(&event)?;
                         if is_ck(&event, kind)? {
                             unseal_event_in_domain(&mut event, &context.domain)?;
                             continue;
                         }
+                        if kind == ZaEventStorageKind::LegacyPlaintext {
+                            eprintln!("Mutation10 skipped legacy plaintext event in conversion: event={}", id);
+                            continue;
+                        }
                         rewrap_event(&mut event, &context.domain, &context.domain)?;
                         write_event(
                             server,
                             guards,
                             changed_by,
                             &context,
                             *id,
@@ -281,15 +285,20 @@
                             ZaEventStorageKind::LegacyPlaintext => counts.sealed += 1,
                             ZaEventStorageKind::Sealed => counts.rewrapped += 1,
                         }
                     }
                 }
                 // No grant-ready result until every current row has sole parent
                 // and an authenticated envelope in this ring's domain.
-                validate_scan(server, &context, true).await?;
+                let checked = validate_scan(server, &context, true).await;
+                if matches!(&checked, Err(ZaShareError::Seal(SealError::NotSealed))) {
+                    let current = read_calendar_for_mutation10(server, keys.account_id, calendar_id).await?;
+                    eprintln!("Mutation10 final forward scan returned NotSealed; calendar={}, ACL count={}", current.name, current.acls.len());
+                }
+                checked?;
                 return Ok(counts);
             }
         }
     }
 }

 pub(super) fn owner_ring(
@@ -514,14 +523,19 @@
         .await?;
     Ok((ids, linked))
 }

 /// At most one logical group's snapshots survive across writes/a pause. Reading
 /// the next group before the between-group seam also makes genuine archive CAS
 /// races observable without holding the entire calendar in memory.
+async fn read_calendar_for_mutation10(server: &Server, owner: u32, calendar: u32) -> Result<Calendar, ZaShareError> {
+    let stored: Archive<AlignedBytes> = server.store().get_value(ValueKey::archive(owner, Collection::Calendar, calendar)).await?.ok_or(SealError::NotSealed)?;
+    Ok(stored.deserialize::<Calendar>()?)
+}
+
 async fn read_chunk(
     server: &Server,
     owner: u32,
     ids: &[u32],
 ) -> Result<Vec<(u32, Archive<AlignedBytes>)>, ZaShareError> {
     let mut chunk = Vec::with_capacity(ids.len());
     for &id in ids {
```

## Mutation 11, attempt 1: survived

Required case: 6 (revoke: refused grantee, new generation, post-revoke event carries it, revoked saved ring lacks it). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `90cb381d43c0872fd727132068292945279180641652ca05606c390d32a610c1`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 134.084s. Decisive observation:
Case 6's fixtures passed under the mutation. The run then failed later at `sharing_races.rs:4148`: `assertion failed: current_ring_generation(&after) > generation_before`, the precondition check of case 22's fixture (grantee_http_write_races, first iteration: Carol revoked while Bob's held CREATE is parked). Whole ZA failed 0 passed/1 failed in 70.37s (build 55.15s).

State evidence: The case-6 revokes in sharing.rs all answered as expected with no rotation: sharing_endpoint_tests' revoke of the reader (sharing.rs:2822-2836: `changed` true, repeat `changed` false, the reader's `shared_with_me` empty), the pin-contract revoke (2963) and the rotation helper's revokes (3125-3293), including the check at 3324 that the remaining grantee's resealed ring lists exactly the owner ring's generations (both unchanged under the mutation, so equal) and the unchanged-event check at 3539. No assertion in those fixtures compares the owner ring's generations before and after a revoke, or checks a revoked grantee's saved ring against the new generation, or the generation of an event written after the revoke; the only `len() + 1` check (3411) follows a same-level re-key, which this patch leaves rotating. The first oracle that saw the condition is case 22's precondition in sharing_races, which runs after sharing.rs.

Ruling: Survived for its required case 6 (unresolved coverage until an oracle correction): the suite failed only after case 6's fixtures passed under the mutation; the later detection at case 22's precondition is recorded but is not the assigned case. The draft was corrected before execution to be faithful to the ledger condition "no rotation on revoke": the original draft also removed the re-key rotation, a different rule (case 8's), so a revoke request now skips the generation push while re-keys and grant-time sweeps still rotate; the affected-grantee invalidation and the resealing of every remaining grant ring are unchanged. Missing oracle, proposed (test-only, existing seams): in sharing_rotation_contracts around the reader revoke at sharing.rs:3288, read `sharing_http_ring(test, &owner_keys, id)` and the revoked reader's grant-ring bytes from its preferences entry before the revoke; after it assert the owner ring's generations grew by exactly one, that `parse_grant_header` of the saved reader ring does not list the new last generation, and, after an owner PUT of a new event into `http-sharing`, that the event's X-ZA-KEY envelope names that newest generation (bytes 6..10, as leak.rs:591 reads it); the existing unchanged-event check at 3539 already covers the events written before. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `962be910efd6129ebc3dc77a466a394f261aadb0a870e1f9499ac19ebd5bb7e5` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -334,16 +334,19 @@
             applied.affected_grantees.push(account_id);
         }
     }
     if !applied.changed {
         return Ok(applied);
     }
     if applied.rotated {
-        header.generations.push(header.next_generation()?);
-        ring.keys.push(*Secret::random().as_bytes());
+        // Mutation11: no rotation on revoke; a revoke keeps the current generations.
+        if revoke.is_none() {
+            header.generations.push(header.next_generation()?);
+            ring.keys.push(*Secret::random().as_bytes());
+        }
         applied
             .affected_grantees
             .extend(ring.grants.iter().map(|g| g.account_id));
     }
     let opened = header
         .generations
         .iter()
```

## Mutation 12, attempt 1: killed

Required case: 8 with remaining-key substitution (a revoke rotates while a remaining grantee's served key differs from its ledger key). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `c51b22008fd09d3c080fa374b9e55a31ed5103d2b7de0c13f075fa28057d4aba`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 253.506s. Decisive observation:
`sharing.rs:3322`: `open_grant_ring(&bytes, owner, id, second_id, domain, &private).unwrap()` panicked with `called Result::unwrap() on an Err value: Aead`. Whole ZA failed 0 passed/1 failed in 63.72s (build 3m00s).

State evidence: In sharing_rotation_contracts, the second grantee's served public key was replaced (substitute_public_key at sharing.rs:3274) while its ledger key stayed the original; the owner's list reported that grant as `key changed` (3285 held), and the revoke of the reader answered 200 `changed: true` (3291 held), rotating and resealing every remaining grant ring. The second grantee's resealed ring, read back from its preferences entry, no longer opens with the original private key that matches the ledger key: AEAD authentication of its pk_wrap records failed, because the mutated seal_grant_ring call wrapped them to the freshly served (substituted) key. Every earlier rotation and grant in the run passed, as expected: there the served key equals the ledger key (a new grant's pin is checked equal to the served key, and a re-key records the newly pinned key).

Ruling: Killed at case 8's remaining-key substitution, the case the ledger names. Replacing `&grant.public_key` (the ledger key) with `za_served_public_key(grant.account_id)` (real signature `async fn(&self, u32) -> trc::Result<Option<[u8; 32]>>`, mapped through the existing From impls; a missing served key would map to NotSealed, which no fixture reaches) in seal_grant_ring makes a rotation wrap to the directory's key instead of the ledger's, and the remaining grantee's own private key no longer opens its ring. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `37fe0497550088edc271318760fce48db1860fc1f6c706715652da4be28a677f` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -361,15 +361,15 @@
                     grant_change_id: grant.change_id,
                     rename_change_id: header.rename_change_id,
                     boundary_pending: grant.change_id == 0,
                     generations: header.generations.clone(),
                 },
                 &opened,
                 keys.account_id,
-                &grant.public_key,
+                &server.za_served_public_key(grant.account_id).await?.ok_or(SealError::NotSealed)?,
             )?;
             if Some(grant.account_id) == new_grant {
                 // The new generation is relevant only when this grant also swept a dead principal.
                 let domain = crate::calendar::seal::ZaKeyDomain::shared(
                     keys.account_id,
                     keys.account_id,
                     Some(keys.ewk().clone()),
```

## Mutation 13, attempt 1: inconclusive for assigned case

Required case: 7 (a grantee granted after rotations receives, and reads, every generation). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `967e5b035cb78328662f18eaf6073919470a8aee1df6fa15719987ec05933845`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 248.892s. Decisive observation:
`utils/za.rs:145` (VaultReply::expect): `unexpected status`, left `500`, right `200`, body `{"type":"about:blank","status":500,"title":"Internal Server Error",...}`, raised in sharing_http_schedule_contracts (cases 9/10, reached from sharing_lifecycle_differential) after its `Case9 own-name organizer` line; the only VaultReply expectations between that line and the function's end are the revoke of Bob at sharing.rs:9280 and the re-grant at 9289. Whole ZA failed 0 passed/1 failed in 48.90s (build 3m09s).

State evidence: Case 7's fixture (sharing_rotation_contracts, sharing.rs:3505-3534, in sharing_endpoint_tests) was not reached. Attribution, from the source (the log carries no trace): the revoke at 9280 is the first rotation of the `lifecycle` calendar and has no remaining grantee, so it is unaffected; the re-grant at 9289 is the run's first new grant on a calendar with two generations. The mutated loop seals that grantee's ring with only the newest generation, and za_share_apply's own pre-commit entry validation, za_replace_calendar_entries (format.rs:637-645), refuses any grant ring whose generations differ from the owner ring's with SealError::Structure("grant header correspondence"); share_error maps that to DataCorruption, hence the 500 and nothing committed. The same refusal would answer case 7's own grant at sharing.rs:3506.

Ruling: Inconclusive for case 7: the run failed with a 500 at an earlier fixture (cases 9/10), not at the assigned case. The condition is caught by a production structural guard on the write path, the grant-header correspondence check, so no newest-only ring can be stored and every new grant to a multi-generation calendar fails the same way, case 7's included. No principled narrowing reaches case 7: the masking re-grant and case 7's grant take the same production path (HTTP grant -> za_share_apply new-grant branch), so reaching case 7 needs either a fixture-specific condition (as M9's slug filter) or bypassing the correspondence check, which is a different rule. Row stopped here and reported to the coordinator without further attempts. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `7f1371f194dd072010abe8611f546c8cbf77742e383d36f12bdb14e37108b4eb` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -349,25 +349,27 @@
         .iter()
         .copied()
         .zip(ring.keys.iter().copied().map(Secret::from_bytes))
         .collect::<BTreeMap<_, _>>();
     // Bundle envelopes remain unchanged across rotation; only a new entry is sealed.
     for grant in &ring.grants {
         if applied.rotated || Some(grant.account_id) == new_grant {
+            let mutation_generations = if Some(grant.account_id) == new_grant { vec![*header.generations.last().unwrap()] } else { header.generations.clone() };
+            let mutation_opened = opened.iter().filter(|(g,_)| mutation_generations.contains(g)).map(|(g,k)|(*g,k.clone())).collect::<BTreeMap<_,_>>();
             let encoded = seal_grant_ring(
                 &GrantRingHeader {
                     calendar_id,
                     domain_id: header.domain_id,
                     grantee_id: grant.account_id,
                     grant_change_id: grant.change_id,
                     rename_change_id: header.rename_change_id,
                     boundary_pending: grant.change_id == 0,
-                    generations: header.generations.clone(),
+                    generations: mutation_generations,
                 },
-                &opened,
+                &mutation_opened,
                 keys.account_id,
                 &grant.public_key,
             )?;
             if Some(grant.account_id) == new_grant {
                 // The new generation is relevant only when this grant also swept a dead principal.
                 let domain = crate::calendar::seal::ZaKeyDomain::shared(
                     keys.account_id,
```

## Mutation 7, attempt 2: killed (supplementary, ruling R11-7a)

Required case: 22 (PUT across a revoke; supplementary to the row-7 kill at 21). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `175ef6184759e6b916d4f0c245a01262154510f10fd6b6dea65ba04efabea3ad`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 166.191s. Decisive observation:
`sharing_races.rs:4191`: `assert_eq!(generation(&opened), current_ring_generation(&after))` failed with left `1`, right `2`, in grantee_http_write_races, the first loop iteration (revoked=false, operation CREATE). Whole ZA failed 0 passed/1 failed in 67.22s (build 1m30s; only the inherited linker eh_frame warning).

State evidence: Every sharing.rs module ran under the narrowed omission and passed, case 21's owner-write fixtures included (the log's last marker is `Running conversion assertion, rollback, resume and cache contracts...`, then sharing_races::test starts with grantee_http_write_races, which prints no marker). In that fixture's first iteration Bob's grantee PUT creating `rotated-create.ics` was parked at COMMIT_PAUSE, Alice revoked Carol (the ring generation advanced, the `>` assertion at 4141 passed), the held create was released and answered 201, and its stored event carries generation 1 while the owner ring is at 2: without the calendar assertion the grantee's create committed under the pre-revoke generation instead of failing its fence and retrying under the current ring. The revoked-Bob 403 sub-variants and the UPDATE/DELETE operations were not reached.

Ruling: Supplementary evidence for row 7, which stays killed at case 21 (attempt 1). Per ruling R11-7a the same physical omission of both `context.assert_in` loops (update at update.rs:482 and create at update.rs:798) was narrowed to writers whose account differs from the calendar's owner (`access_token.account_id() != account_id`, both values already at the seam); the owner's writes keep the original loops byte-for-byte, the event archive and UID lease assertions are untouched. Case 22's grantee write across a revoke-driven rotation commits under the stale generation, which its exact generation oracle detects: killed at 22, rotated variant (the fixture's held CREATE runs first; spec 11.2 case 22 names updates, and the fixture extends the same assertion to the held create). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `f68f5d3990f3632272af5fd54dcd3ce31e326a8a288489b621b31d3ba5c19f9b` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -475,16 +475,19 @@

             // The same event/index/alarm intent is rebuilt once, without scheduling.
             let schedule_tag = new_event.schedule_tag;
             let timestamp = store::write::now() as i64;
             let mut etag;
             for attempt in 0..=1 {
                 let mut batch = BatchBuilder::new();
-                for context in &contexts {
-                    context.assert_in(&mut batch);
+                if access_token.account_id() == account_id {
+                    // Mutation7 (R11-7a): omitted for grantee writers only.
+                    for context in &contexts {
+                        context.assert_in(&mut batch);
+                    }
                 }
                 etag = new_event
                     .clone()
                     .update_at(
                         access_token.account_tenant_ids(),
                         event_.to_unarchived::<CalendarEvent>()?,
                         account_id,
@@ -791,16 +794,19 @@
                 |event: &CalendarEvent,
                  contexts: &[groupware::calendar::sharing::resolve::ZaCalendarContext]|
                  -> trc::Result<(BatchBuilder, Option<String>)> {
                     let mut batch = BatchBuilder::new();
                     if let Some((lease, _)) = &za_lease {
                         lease.lease().assert_in(&mut batch);
                     }
-                    for context in contexts {
-                        context.assert_in(&mut batch);
+                    if access_token.account_id() == account_id {
+                        // Mutation7 (R11-7a): omitted for grantee writers only.
+                        for context in contexts {
+                            context.assert_in(&mut batch);
+                        }
                     }
                     let etag = event
                         .clone()
                         .insert_at(
                             access_token.account_tenant_ids(),
                             account_id,
                             document_id,
```

## Mutation 13, attempt 2: inconclusive for assigned case

Required case: 7 (a grantee granted after rotations receives, and reads, every generation). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `5a6298e7569c2a4cf4defe08d3b8911b2f190f0fc545528ba12bdcb96721548b`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 121.815s. Decisive observation:
`utils/za.rs:145` (VaultReply::expect): `unexpected status`, left `500`, right `200`, body `{"type":"about:blank","status":500,"title":"Internal Server Error",...}`, immediately after the trace line `Mutation13 entry validation refused the share: calendar=http-sharing, new_grant=Some(34), generations=[1, 2], error=Structure("grant header correspondence")`, under the marker `Running transactional sharing endpoint tests...`. Whole ZA failed 0 passed/1 failed in 61.80s (build 52.12s).

State evidence: The scope by stored `Calendar.name == "http-sharing"` does not isolate case 7: that calendar is shared by case 3 (its first grant, sharing.rs:2757, generations [1]), sharing_pin_contracts (sharing.rs:2853-2998) and case 7's grant in sharing_rotation_contracts (sharing.rs:3507), and case 7's grantee (`shares-http-reader`) is the same account that sharing_pin_contracts revokes and re-grants. The traced refusal has generations [1, 2], the calendar's state after its first rotation: sharing_pin_contracts' revoke of the reader (sharing.rs:2965) rotated it, and its re-grant (sharing.rs:2976, `expect(200)`) is the first new grant on a two-generation `http-sharing`; the mutated loop sealed that grant ring with only generation 2, and za_share_apply's pre-commit entry validation, za_replace_calendar_entries (format.rs:637-645), refused it with SealError::Structure("grant header correspondence"), mapped by share_error to DataCorruption (500); the error is returned before the batch is built, so nothing was committed and no grant was published. Case 7 (sharing.rs:3505-3534) was not reached.

Ruling: Inconclusive for case 7 under ruling R11-13: the first failure is at an earlier fixture on the same calendar (sharing_pin_contracts' re-grant at sharing.rs:2976), not at case 7's grant. The trace is observation-only (the same error is returned) and shows the production correspondence guard, format.rs:637-645, refusing the newest-only ring; the same guard would refuse case 7's own grant the same way, so the production guard preempts case 7's reader oracle: no newest-only grant ring can be stored. The two grants differ in nothing at the seam but the calendar's generation count and their order (same calendar, same grantee, same new-grant branch of za_share_apply), so reaching case 7 needs a generation-count or call-order condition, a fixture-specific shaping beyond the approved calendar-name scope. Row stopped and reported to the coordinator without further attempts. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `261fb86439bebb8e61ddddb7eaa691511bfa63326540ae828b083e347e49eac4` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -349,25 +349,28 @@
         .iter()
         .copied()
         .zip(ring.keys.iter().copied().map(Secret::from_bytes))
         .collect::<BTreeMap<_, _>>();
     // Bundle envelopes remain unchanged across rotation; only a new entry is sealed.
     for grant in &ring.grants {
         if applied.rotated || Some(grant.account_id) == new_grant {
+            // Mutation13 (R11-13): newest-generation-only new grant, case 7's calendar only.
+            let mutation_generations = if Some(grant.account_id) == new_grant && calendar.name == "http-sharing" { vec![*header.generations.last().unwrap()] } else { header.generations.clone() };
+            let mutation_opened = opened.iter().filter(|(g,_)| mutation_generations.contains(g)).map(|(g,k)|(*g,k.clone())).collect::<BTreeMap<_,_>>();
             let encoded = seal_grant_ring(
                 &GrantRingHeader {
                     calendar_id,
                     domain_id: header.domain_id,
                     grantee_id: grant.account_id,
                     grant_change_id: grant.change_id,
                     rename_change_id: header.rename_change_id,
                     boundary_pending: grant.change_id == 0,
-                    generations: header.generations.clone(),
+                    generations: mutation_generations,
                 },
-                &opened,
+                &mutation_opened,
                 keys.account_id,
                 &grant.public_key,
             )?;
             if Some(grant.account_id) == new_grant {
                 // The new generation is relevant only when this grant also swept a dead principal.
                 let domain = crate::calendar::seal::ZaKeyDomain::shared(
                     keys.account_id,
@@ -420,15 +423,23 @@
             Ok(AclGrant {
                 account_id: g.account_id,
                 grants: ZaShareLevel::from_code(g.level)?.bitmap(),
             })
         })
         .collect::<Result<Vec<_>, SealError>>()?;
     let entries = calendar.preferences.clone();
-    za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
+    let mutation13_checked =
+        za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls);
+    if let Err(error) = &mutation13_checked {
+        eprintln!(
+            "Mutation13 entry validation refused the share: calendar={}, new_grant={:?}, generations={:?}, error={:?}",
+            calendar.name, new_grant, header.generations, error
+        );
+    }
+    mutation13_checked?;
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut batch = BatchBuilder::new();
     context.assert_in(&mut batch);
     guards.assert_in(&mut batch);
```

## Mutation 14, attempt 1: killed

Required case: 11 (resolver eligibility: a planted group grant does not admit its member). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `23ad3c523148adcd419f46169e72a7555215446fb58c8a45b450c617186f8f27`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 102.286s. Decisive observation:
`sharing.rs:8348`: `assert!(!result.contains("sharing-dav-alice"), "ineligible discovery: {result}")` failed for the group member grace (`sharing-gate-grace`), on the depth-1 root discovery `PROPFIND /dav/cal/` (allprop): the 207 lists `/dav/cal/sharing-dav-alice%40example.com/` and `/dav/pal/sharing-dav-alice%40example.com/` beside grace's own and the group's homes. Whole ZA failed 0 passed/1 failed in 44.06s (build 50.63s).

State evidence: In grantee_gate_controls (case 11, run from grantee_dav_admission), the fixture plants on Alice's `private` calendar an ACL entry for the group `sharing-gate-group` (Read, ReadItems) with no grant-ring entry, and makes grace a member. Under the mutation grace's direct `PROPFIND {home}private/` still answered 403 (sharing.rs:8224 passed): authorize_calendar's eligibility and ACL-bit checks now accept the group's entry through is_member, but its third check requires a preference entry under grace's own id, which the planted state lacks, so the resolver still forbids. Grace's PROPFIND of Alice's home (207) and Bob's own-home and root discovery passed. Grace's own-home listing passed, then her root discovery failed: common/propfind.rs:286-287 keeps a foreign key-account home in a root listing only if foreign_home_visible (principal/propfind.rs:398-420) finds a calendar for which za_foreign_calendar_eligible holds, and the mutated predicate accepts grace through the group's ACL entry, so Alice's home was listed.

Ruling: Killed at case 11. The patch is the draft, unchanged: the exact own-id comparison becomes token.is_member(grant.account_id) in both of the resolver module's principal checks, za_foreign_calendar_eligible (resolve.rs:129) and authorize_calendar's required-bits check (resolve.rs:161); the eligibility predicate is the resolver's own admission rule (authorize_calendar calls it first) and the one discovery shares. The detecting observation is the case-11 discovery oracle that a planted group grant leaves Alice's home out of its member's listings. As the handoff warned, the direct-access half is masked: grace's direct PROPFIND stayed 403 because authorize_calendar's preference-entry check under the token's own id also refuses (and resolve_stored_calendar looks the entry up by the token's own id), so the is_member change in the required-bits check alone has no observable effect on this fixture; admitting a member would need a preference entry under the member's own id with only a group ACL entry, a state the scanner's ACL/preference correspondence rule rejects. No oracle change is proposed since the row is killed. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/resolve.rs` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` | `452785143797645f0abee4f23ab6aec0681cd979818506d01d8f14c9e66ea997` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/resolve.rs
+++ b/crates/groupware/src/calendar/sharing/resolve.rs
@@ -122,15 +122,15 @@
     !token.za_is_impersonated()
         && !token.has_permission(Permission::Impersonate)
         && token
             .session_keys()
             .is_some_and(|keys| keys.account_id == token.account_id())
         && grants
             .iter()
-            .any(|grant| grant.account_id == token.account_id() && grant.grants.contains(Acl::Read))
+            .any(|grant| token.is_member(grant.account_id) && grant.grants.contains(Acl::Read))
 }

 fn forbid(owner_id: u32) -> ZaCalendarError {
     trc::event!(
         Security(trc::SecurityEvent::Unauthorized),
         AccountId = owner_id,
         Details = "zero-access: calendar access without a collection key"
@@ -154,15 +154,15 @@
 ) -> Result<(), ZaCalendarError> {
     session(token, owner_id)?;
     if token.account_id() == owner_id {
         return Ok(());
     }
     if !za_foreign_calendar_eligible(token, &calendar.acls)
         || !calendar.acls.iter().any(|grant| {
-            grant.account_id == token.account_id()
+            token.is_member(grant.account_id)
                 && grant.grants.bitmap & required.bitmap == required.bitmap
         })
         || !calendar
             .preferences
             .iter()
             .any(|entry| entry.account_id == token.account_id())
     {
```

## Mutation 15, attempt 1: survived

Required case: 11 (resolver refuses a grantee whose own grant lacks the required bits). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `005797935ea68d095e9025a0971d99ee64ed6b86b499876a6cc7e59f63942c42`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `0`, elapsed wall time 419.983s. Decisive observation:
No assertion failed: `test result: ok. 1 passed; 0 failed` in 368.73s (build 42.51s; new warning `unused variable: required` in authorize_calendar, expected from the patch), strict final cleanup included.

State evidence: Every module ran under the mutation, case 11 (grantee_gate_controls) and the Task 10 method matrix (grantee_dav_admission: view Bob's PUT, DELETE and PROPPATCH refused, nothing stored) included. Every DAV writer checks the upstream container ACL from the request's DAV resource snapshot before it calls the resolver (for example update.rs:161-165, `resources.has_access_to_container(access_token, parent_id, Acl::ModifyItems)`, which compares the same calendar ACL bits), so a view grantee is refused before authorize_calendar is reached and its required-bits check is redundant on every unraced path the suite drives; the planted group grant and the unshared calendars are refused by the own-id eligibility and preference checks, which the patch keeps.

Ruling: Survived: unresolved coverage, not an equivalence. The resolver's required-bits check is the only fresh check of the grantee's level: the upstream check reads the DAV resource snapshot fetched at request start (update.rs:90-97), before PUT_PAUSE (update.rs:99). Proposed oracle (test-only, existing seams, no production change): in the case-11 or case-22 fixture, park an edit grantee's PUT update of an existing shared event with the existing PUT_PAUSE hook (`test_hooks::set_put_pause`, keyed on the owner's account id, already used by uid_lease.rs:62), change that grantee's level to view through `shares/grant` with `level: view`, release, and require 403 with the stored event archive byte-for-byte unchanged; under this mutation the fresh resolver accepts the downgraded grant and the PUT commits (204). The existing groupware unit `calendar::sharing::resolve::tests::resolver_requires_own_grant_and_all_bits` (resolve.rs:794-855, view reader with ModifyItems required must be Forbidden) also targets this exact predicate and can serve as a supplementary direct oracle, not a replacement for the HTTP case. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/resolve.rs` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` | `e1997ef4e5a9c9a7664a92c4a11ea09d6e9dee9cc1dc287e6ed4e8476b9c9357` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/resolve.rs
+++ b/crates/groupware/src/calendar/sharing/resolve.rs
@@ -155,15 +155,14 @@
     session(token, owner_id)?;
     if token.account_id() == owner_id {
         return Ok(());
     }
     if !za_foreign_calendar_eligible(token, &calendar.acls)
         || !calendar.acls.iter().any(|grant| {
             grant.account_id == token.account_id()
-                && grant.grants.bitmap & required.bitmap == required.bitmap
         })
         || !calendar
             .preferences
             .iter()
             .any(|entry| entry.account_id == token.account_id())
     {
         return Err(forbid(owner_id));
```

## Mutation 16, attempt 1: killed

Required case: 11 (a refused calendar is skipped in a listing: the home PROPFIND succeeds and lists only eligible calendars). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `e8b0c5afa7f9b5ad600cac939a0a67b08646ddc26bb23e88aa24e137e6f9b241`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 165.676s. Decisive observation:
`utils/webdav.rs:599` (`with_status`): `Expected 207 Multi-Status but got 403 Forbidden`, empty body, under the marker `Running Task 10 grantee DAV admission and method matrix...`, before grantee_sync_boundaries' marker. Whole ZA failed 0 passed/1 failed in 42.49s (build 1m55s).

State evidence: In grantee_dav_admission (the Task 10 admission matrix that hosts case 11's grantee_gate_controls), Alice owns `shared` (granted to Bob at view) and the unshared `private`. Bob's direct GET, HEAD and PROPFIND of `shared/event.ics` passed (they resolve only the requested calendar). The first request that resolves every calendar of Alice's home is Bob's depth-1 allprop `PROPFIND` of Alice's home (sharing.rs:6692-6698, required 207, then `/shared/` present and `/private/` absent): the propfind `get` path calls foreign_calendar_contexts (common/propfind.rs:1308-1343) with no requested calendar, the resolver refuses `private` (no own-id grant) with ZaCalendarError::Forbidden, and the mutated arm returns it as 403 for the whole request instead of omitting `private`. The panic carries no test line; the request is identified from the source order (earlier requests address only `shared`), the absent later marker and the empty 403 body that za_calendar_error(Forbidden) produces.

Ruling: Killed at case 11 ("Bob reaches none of alice's unshared calendars (403 when addressed, absent from listings)" and "the resolver skips the refused calendar"): the required 207 of a home listing failed because the refusal of one ineligible calendar aborted the request, which is the row's condition exactly. The patch is the draft, unchanged: only the Forbidden arm of foreign_calendar_contexts returns `crate::calendar::za_calendar_error(ZaCalendarError::Forbidden, owner_id, id)` (helper confirmed at calendar/mod.rs:363, signature (ZaCalendarError, u32, u32) -> DavError, mapping Forbidden to 403); the NotFound skip, the other error arm and the downstream per-path skip at common/propfind.rs:435 are unchanged. The detection came in the matrix fixture before grantee_gate_controls' own-home discovery loop was reached. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `9f5946b21861949f545a9dd106e8beb6a3dc1cb67963e033beb1e491279ae7d3` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1326,15 +1326,15 @@
         )
         .await
         {
             Ok(Some(context)) => {
                 eligible.insert(id, Arc::new(context));
             }
             Ok(None) => (),
-            Err(ZaCalendarError::Forbidden) => (),
+            Err(ZaCalendarError::Forbidden) => return Err(crate::calendar::za_calendar_error(ZaCalendarError::Forbidden, owner_id, id)),
             Err(ZaCalendarError::Store(error))
                 if error.matches(trc::EventType::Store(trc::StoreEvent::NotFound)) =>
             {
                 ()
             }
             Err(error) => return Err(crate::calendar::za_calendar_error(error, owner_id, id)),
         }
```

## Mutation 17, attempt 1: killed

Required case: 11 (the gate refuses a Bearer token without its own session keys). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `9f3d720b44dc4ac0f61d6be2e736317dd28cd96e8655b38a23f19ecbe69ed48d`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 170.622s. Decisive observation:
`sharing.rs:8250`: `assert_eq!(response.status, StatusCode::FORBIDDEN, "{label}: foreign home must be refused at URI gate")` failed for label `keyless`, left `207`, right `403`. Whole ZA failed 0 passed/1 failed in 44.31s (build 1m58s).

State evidence: In grantee_gate_controls, Bob's keyless Bearer token (minted with encode_access_token, no session keys) first passed the masked checks under the mutation: its PROPFIND of Bob's own address book answered 207 and its GET of `shared/event.ics` still answered 403, because the fresh resolver's session() check refuses a keyless token with the resolver's own reason. Then the gate-specific oracle: the keyless token's PROPFIND of Alice's home root, captured with Traced, answered 207 instead of 403: the mutated gate admitted the token (not impersonated, no Impersonate permission, shared access to Alice's calendars), and the home listing's foreign_calendar_contexts skipped every calendar the resolver refused, returning the root with no eligible children. The trace assertion that follows (the fixed gate detail `zero-access: calendar access without session keys`) was not reached.

Ruling: Killed at case 11 by the gate-specific oracle the live-preparation note prescribes for this row (a foreign-home root PROPFIND that must be 403 at the URI gate), not by a downstream GET 403, which the resolver masks and which passed under the mutation. The patch is the draft, unchanged: only the own-session-keys conjunct (`token.session_keys().is_some_and(|keys| keys.account_id == token.account_id())`) of za_calendar_gate's grantee admission is removed; the impersonation, Impersonate-permission and shared-access conjuncts, the early owner-keys return and the refusal trace are unchanged. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/za.rs` | `da1a2355ee20d9576704dc4ae17e9a1d9a39030447f181d87b5f96b53a13fa2e` | `5d4e4ab6f460c9e1a0f6756eac1103b590e459e2ab1175ff8226ebb3db25eb06` | `da1a2355ee20d9576704dc4ae17e9a1d9a39030447f181d87b5f96b53a13fa2e` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/za.rs
+++ b/crates/dav/src/common/za.rs
@@ -71,18 +71,15 @@
 }

 impl ZeroAccessGate for Server {
     async fn za_calendar_gate(&self, token: &AccessToken, owner_id: u32) -> crate::Result<()> {
         if !is_key_account(self, owner_id).await? || token.za_keys_for(owner_id).is_some() {
             return Ok(());
         }
-        if token
-            .session_keys()
-            .is_some_and(|keys| keys.account_id == token.account_id())
-            && !token.za_is_impersonated()
+        if !token.za_is_impersonated()
             && !token.has_permission(Permission::Impersonate)
             && token.has_shared_access(owner_id, Collection::Calendar)
         {
             return Ok(());
         }
         trc::event!(
             Security(trc::SecurityEvent::Unauthorized),
```

## Mutation 18, attempt 1: killed

Required case: 11 (the gate refuses a grantee whose token carries the Impersonate permission). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `020c2a1738ccee09683aa1d42e4d0b2208219deaf8d6ce3fd059166ea83948fb`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 85.105s. Decisive observation:
`sharing.rs:8250`: `assert_eq!(response.status, StatusCode::FORBIDDEN, "{label}: foreign home must be refused at URI gate")` failed for label `Impersonate`, left `207`, right `403`. Whole ZA failed 0 passed/1 failed in 44.66s (build 32.84s; new warning `unused import: registry::schema::enums::Permission` in dav/common/za.rs, expected from the patch).

State evidence: In grantee_gate_controls, `sharing-gate-imp` is a key account whose role grants Permission::Impersonate, pinned and granted `edit` on Alice's `shared` calendar; its own-home PROPFIND answered 207. In the gate-oracle loop the keyless label passed first (still refused at the gate with its fixed trace, since the patch keeps the own-keys conjunct). For the Impersonate label, the Traced PROPFIND of Alice's home root answered 207 instead of 403: the mutated gate admitted the token (own session keys, not an impersonation copy, shared access to Alice's calendars), and the home listing skipped the calendars the resolver then refused (za_foreign_calendar_eligible still rejects the Impersonate permission), returning the root with no eligible children. The trace assertion and the per-method 403 loop after it were not reached.

Ruling: Killed at case 11 by the gate-specific oracle (foreign-home root PROPFIND required 403 at the URI gate); a downstream GET 403 alone would be masked by the resolver. Audited and narrowed from the draft before preparation: the draft removed both `!token.za_is_impersonated()` and `!token.has_permission(Permission::Impersonate)`, but the ledger condition is the Impersonate permission only, and the impersonation-copy conjunct is a separate rule (case 11's impersonation copy, refused for both the URI gate and the resolver), so the executed patch removes only the permission conjunct of za_calendar_gate's grantee admission; the own-keys, impersonation-copy and shared-access conjuncts, the early owner-keys return and the refusal trace are unchanged. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/za.rs` | `da1a2355ee20d9576704dc4ae17e9a1d9a39030447f181d87b5f96b53a13fa2e` | `1bf8b3e79484de56efbcb999971d0f066ff2f3b1ce292f5ed2ab7d9597f8571b` | `da1a2355ee20d9576704dc4ae17e9a1d9a39030447f181d87b5f96b53a13fa2e` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/za.rs
+++ b/crates/dav/src/common/za.rs
@@ -75,15 +75,14 @@
         if !is_key_account(self, owner_id).await? || token.za_keys_for(owner_id).is_some() {
             return Ok(());
         }
         if token
             .session_keys()
             .is_some_and(|keys| keys.account_id == token.account_id())
             && !token.za_is_impersonated()
-            && !token.has_permission(Permission::Impersonate)
             && token.has_shared_access(owner_id, Collection::Calendar)
         {
             return Ok(());
         }
         trc::event!(
             Security(trc::SecurityEvent::Unauthorized),
             AccountId = owner_id,
```

## Mutation 19, attempt 1: inconclusive for assigned case

Required case: 11 (with assisted discovery on, a grantee's own-home listing shows only eligible foreign calendars). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `0d57848f00ce3767b4b1770ca82d4b50326fcd6f0ded65dbcd410de0fbee60f7`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 185.93s. Decisive observation:
`utils/webdav.rs:120`: `called Result::unwrap() on an Err value: reqwest::Error { kind: Request, url: "https://127.0.0.1:8899/dav/itip/key2%40example.com/inbox/29.ics", source: TimedOut }`, under the marker `Running zero-access drain reply tests...` (drain.rs:424). Whole ZA failed 0 passed/1 failed in 49.59s (build 1m54s).

State evidence: The run stopped in the release-2 drain reply module, long before the sharing modules; the test client's 500 ms request timeout (utils/webdav.rs:100) expired on a GET of an iTIP inbox item. A GET does not pass through the mutated block (the assisted-discovery branch of the PROPFIND `get` path), and no earlier module lists another key account's calendars through assisted discovery in a way that leaves server state behind, so the timeout is not attributed to the condition.

Ruling: Inconclusive for case 11: a client timeout in an earlier, unrelated module, not a kill. Seam audited before preparation: the draft targeted principal/propfind.rs's foreign_home_visible (the eligibility filter of `calendar-home-set` with assisted discovery off and of root discovery, which row 14's run exercised), not the assisted listing; per spec 5.2 and 5.4 the assisted listing is the assisted-discovery block of `get` in crates/dav/src/common/propfind.rs, so the patch disables only that block's eligibility branch (`foreign_eligible` from foreign_calendar_contexts), letting a key account's containers through upstream's shared_containers under is_member as the pre-fork block did. Rerun as attempt 2 with the same patch. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `00e9c1aeb913a71e80a1f40444d586baf68fcc6bb16fbaa2fed3946a1762cc5a` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1752,15 +1752,17 @@
                     if shared_account_id == access_token.account_id() {
                         continue;
                     }
                     let shared_resources = data
                         .resources(server, access_token, shared_account_id, sync_collection)
                         .await
                         .caused_by(trc::location!())?;
-                    let foreign_eligible = if sync_collection == SyncCollection::Calendar
+                    // Mutation19: the assisted listing skips the foreign eligibility check.
+                    let foreign_eligible = if false
+                        && sync_collection == SyncCollection::Calendar
                         && server
                             .za_is_key_account(shared_account_id)
                             .await
                             .caused_by(trc::location!())?
                     {
                         Some(
                             foreign_calendar_contexts(
```

## Mutation 19, attempt 2: survived

Required case: 11 (with assisted discovery on, a grantee's own-home listing shows only eligible foreign calendars). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `0d57848f00ce3767b4b1770ca82d4b50326fcd6f0ded65dbcd410de0fbee60f7`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 138.595s. Decisive observation:
Case 11 passed under the mutation; the run failed later at `utils/webdav.rs:120`: `reqwest::Error { kind: Request, url: "https://127.0.0.1:8899/dav/cal/sharing-life-alice%40example.com/lifecycle/subaddress-owner.ics", source: TimedOut }` under the marker `Running case25 HTTP owner/delegated lifecycle differential...`. Whole ZA failed 0 passed/1 failed in 106.64s (build 21.29s). The host was heavily loaded by unrelated work during this run and attempt 1 (load average about 560, Xcode builds and simulators), against the client's 500 ms request timeout.

State evidence: grantee_dav_admission ran to its end (grantee_gate_controls is its last call, sharing.rs:6854) and the next function, sharing_lifecycle_differential, printed its marker, so every case-11 assertion passed under the mutation: in both assisted-discovery modes, for Bob, the group member grace and the Impersonate holder, with allprop and propname, the own-home and root listings did not name Alice's ineligible calendars and Bob's showed exactly `shared`. With the block's eligibility branch disabled, the assisted block appends Alice's containers through upstream's shared_containers under is_member (grace through the planted group's ACL entry; for the Impersonate holder is_member is true for every account, so all of Alice's containers), but every appended path is then resolved again before emission in the same request: the propname loop calls za_read_context per foreign key-account path (common/propfind.rs:407-445) and the property path calls za_archive_view (common/propfind.rs:536-549); both reach the resolver, whose authorize_calendar applies za_foreign_calendar_eligible first, and a home listing skips a Forbidden path (`if za_home_listing => continue`). The final timeout is in an unrelated later module and does not change this.

Ruling: Survived at case 11: unresolved until the coordinator rules. Seam: the assisted-discovery block of `get` in crates/dav/src/common/propfind.rs (spec 5.2, 5.4), not the draft's principal/propfind.rs foreign_home_visible (see attempt 1); same patch as attempt 1, rerun after its unrelated timeout. Analysis: spec 5.2 places the one eligibility check at three points (before the assisted block appends a foreign path, before a propname response lists one, and in the resolver's grantee arm); the block's own check is redundant with the per-path loader-view checks in the same request, so no HTTP response of the case-11 fixture can differ with it removed. Proposed resolution, in order of preference: (1) record observational equivalence for this single omission, on the argument above (every path the assisted block appends passes za_read_context or za_archive_view before emission, both call the resolver, which applies the same predicate first, and a home listing omits a refused path), with the coordinator's ruling; (2) if an observing oracle is wanted instead, a test-only one at an existing seam: the resolver's test_mode read log (`super::test::record_read` in resolve.rs read_calendar) or the resolver's fixed Unauthorized detail `zero-access: calendar access without a collection key` captured with Traced, comparing which of Alice's calendars the grace own-home assisted listing resolves; I have not established that either differs between original and mutant (the original's foreign_calendar_contexts also resolves every calendar of the shared resources), so (2) needs checking before adoption. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `00e9c1aeb913a71e80a1f40444d586baf68fcc6bb16fbaa2fed3946a1762cc5a` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1752,15 +1752,17 @@
                     if shared_account_id == access_token.account_id() {
                         continue;
                     }
                     let shared_resources = data
                         .resources(server, access_token, shared_account_id, sync_collection)
                         .await
                         .caused_by(trc::location!())?;
-                    let foreign_eligible = if sync_collection == SyncCollection::Calendar
+                    // Mutation19: the assisted listing skips the foreign eligibility check.
+                    let foreign_eligible = if false
+                        && sync_collection == SyncCollection::Calendar
                         && server
                             .za_is_key_account(shared_account_id)
                             .await
                             .caused_by(trc::location!())?
                     {
                         Some(
                             foreign_calendar_contexts(
```

## Mutation 13, attempt 3: killed

Required case: 7 (a grantee granted after rotations receives, and reads, every generation). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `4b4d25b6fd29f7bedcaa1685f802ba52968efd70d451a76845186792e3c27dd3`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 206.583s. Decisive observation:
`utils/za.rs:145` (VaultReply::expect): `unexpected status`, left `500`, right `200`, body `{"type":"about:blank","status":500,"title":"Internal Server Error",...}`, raised by case 7's grant `za_post("shares/grant", &request(READER, "view")).await.expect(200)` (sharing.rs:3507-3509), immediately after the trace lines `Mutation13 new grant on http-sharing: new_grant=Some(34), generations=[1, 2, 3, 4, 5, 6, 7, 8, 9], scoped=true` and `Mutation13 entry validation refused the share: calendar=http-sharing, new_grant=Some(34), generations=[1, 2, 3, 4, 5, 6, 7, 8, 9], error=Structure("grant header correspondence")`, under the marker `Running transactional sharing endpoint tests...`. Whole ZA failed 0 passed/1 failed in 63.72s (build 2m15s).

State evidence: Mapping of every new grant on `http-sharing` before case 7, from the source and confirmed by the observation-only trace (grantee 34 = shares-http-reader, 38 = shares-http-second; count = generations the new grant sees): case 3 sharing.rs:2757 reader [1]; sharing_pin_contracts re-grant 2976 reader [1,2]; sharing_rotation_contracts 3120 second [1,2], 3132 reader (alias address) [1..3], 3142 reader (alias address) [1..4], 3194 second (alias address) [1..5], 3265 second [1..6]; then the revoke at 3290 (7), the same-level re-key of second at 3405 (8, a rotation, not a new grant) and the revoke at 3492 (9); case 7's grant 3507 reader [1..9]. All seven earlier new grants traced `scoped=false` and succeeded (their `expect(200)`s passed); case 7's grant is the first and only one traced `scoped=true`. Every other row of the ledger loop and every other grant kept the original full generation vector. The mutated loop sealed case 7's grant ring with only generation 9; za_share_apply's pre-commit entry validation, za_replace_calendar_entries (format.rs:637-645), refused it with SealError::Structure("grant header correspondence"), which share_error maps to DataCorruption (500). The error is returned before the batch is built, so nothing was committed and no grant was published; the test panicked at the grant's own expect(200), so the fixture's reader-side oracle (the grant header's generations equal the owner vector, sharing.rs:3511-3533) was not reached.

Ruling: Killed at case 7 under ruling R11-13b: the condition (a new grantee given only the newest generation) is scoped by keys available at the seam, calendar name `http-sharing` plus a generation count of at least 7, which the mapping shows makes case 7's grant the first grant the condition affects (earlier new grants on that calendar see at most 6 generations, and the trace confirms every one ran unaffected). Case 7's required 200 was refused with the traced "grant header correspondence" origin and no grant published. The production guard preempts case 7's reader oracle: the write path's correspondence check (format.rs:637-645) refuses every grant ring whose generations differ from the owner ring's, so a newest-only grant can never be stored and the fixture's own header-generations assertion is not what detects it. The traces are observation-only (the same error is returned). Attempts 1 and 2 were inconclusive (earlier fixtures on multi-generation calendars hit the same guard first). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `0278391a32d98684b330a23fcbf41e42e41b3c2605d4ca1eb9e7b0c3acba9c8a` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -349,25 +349,29 @@
         .iter()
         .copied()
         .zip(ring.keys.iter().copied().map(Secret::from_bytes))
         .collect::<BTreeMap<_, _>>();
     // Bundle envelopes remain unchanged across rotation; only a new entry is sealed.
     for grant in &ring.grants {
         if applied.rotated || Some(grant.account_id) == new_grant {
+            // Mutation13 (R11-13b): newest-generation-only new grant, case 7's grant only
+            // (calendar http-sharing with at least 7 generations; earlier new grants there see at most 6).
+            let mutation_generations = if Some(grant.account_id) == new_grant && calendar.name == "http-sharing" && header.generations.len() >= 7 { vec![*header.generations.last().unwrap()] } else { header.generations.clone() };
+            let mutation_opened = opened.iter().filter(|(g,_)| mutation_generations.contains(g)).map(|(g,k)|(*g,k.clone())).collect::<BTreeMap<_,_>>();
             let encoded = seal_grant_ring(
                 &GrantRingHeader {
                     calendar_id,
                     domain_id: header.domain_id,
                     grantee_id: grant.account_id,
                     grant_change_id: grant.change_id,
                     rename_change_id: header.rename_change_id,
                     boundary_pending: grant.change_id == 0,
-                    generations: header.generations.clone(),
+                    generations: mutation_generations,
                 },
-                &opened,
+                &mutation_opened,
                 keys.account_id,
                 &grant.public_key,
             )?;
             if Some(grant.account_id) == new_grant {
                 // The new generation is relevant only when this grant also swept a dead principal.
                 let domain = crate::calendar::seal::ZaKeyDomain::shared(
                     keys.account_id,
@@ -420,15 +424,29 @@
             Ok(AclGrant {
                 account_id: g.account_id,
                 grants: ZaShareLevel::from_code(g.level)?.bitmap(),
             })
         })
         .collect::<Result<Vec<_>, SealError>>()?;
     let entries = calendar.preferences.clone();
-    za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
+    if new_grant.is_some() && calendar.name == "http-sharing" {
+        eprintln!(
+            "Mutation13 new grant on http-sharing: new_grant={:?}, generations={:?}, scoped={}",
+            new_grant, header.generations, header.generations.len() >= 7
+        );
+    }
+    let mutation13_checked =
+        za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls);
+    if let Err(error) = &mutation13_checked {
+        eprintln!(
+            "Mutation13 entry validation refused the share: calendar={}, new_grant={:?}, generations={:?}, error={:?}",
+            calendar.name, new_grant, header.generations, error
+        );
+    }
+    mutation13_checked?;
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut batch = BatchBuilder::new();
     context.assert_in(&mut batch);
     guards.assert_in(&mut batch);
```

## Mutation 20, attempt 1: killed

Required case: 9 (the drain's delegated branch refuses a planted item whose SENT-BY does not resolve to its changed_by). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `e4552cb1bc6c69aba540b6a6737ec972ca9b79a5719ab2e346441a9c9b8307ed`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 117.43s. Decisive observation:
`sharing.rs:5567` (delegated_receiver_rejections_and_reply): `assertion left == right failed: wrong`, left `1`, right `0`: `uid_copies(test, writer.id, &uid)` for the planted item labelled `wrong`. Whole ZA failed 0 passed/1 failed in 52.06s (build 57.53s); the last suite marker is `Running case25 HTTP owner/delegated lifecycle differential...` (case 25 and the whole lifecycle differential, delegated_receiver_consistency included, passed: they print no marker of their own after it).

State evidence: The fixture plants, in the writer's (key2) inbox, a pending REQUEST whose ORGANIZER names key1 (the owner) with `SENT-BY="mailto:key3@example.com"` and `changed_by` = key2, then drains. The first planted item, `missing` (no SENT-BY), took the unmutated non-delegated branch and was refused as before (0 copies). For `wrong` the mutated delegated branch skipped the SENT-BY-to-changed_by comparison, and its remaining conjuncts held (the named key1 differs from changed_by key2, and both are key accounts), so the item verified, `import` ran and the writer's calendar holds one copy of UID `task9-receiver-wrong` instead of none. The fixed refusal `zero-access: delegated sender not verified` the fixture asserts next (sharing.rs:5584-5590) was therefore never emitted; the panic at the copy count precedes that assertion.

Ruling: Killed at case 9: the assertion is the case's own `no import` oracle for the forged-SENT-BY item and the condition alone explains it (an item whose SENT-BY names someone other than its changed_by is accepted and imported). The patch short-circuits only the `sent_by_id == Some(*sender_id)` conjunct (`true || ...`), keeping both directory lookups, the named-differs-from-sender check and both key-account checks, and leaves the non-delegated branch untouched. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/drain.rs` | `a7bf9e4593201c5b698dcee89cc6fbd36d49d5cc81225333da1d85a0ae7a8024` | `5bde01f98a11bf63326c9eed71b6a10c87380dfbacc476b38d32617489719bcf` | `a7bf9e4593201c5b698dcee89cc6fbd36d49d5cc81225333da1d85a0ae7a8024` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/drain.rs
+++ b/crates/groupware/src/calendar/drain.rs
@@ -659,15 +659,16 @@
                         .za_account_id_from_email(sent_by)
                         .await
                         .caused_by(trc::location!())?;
                     let named_id = server
                         .za_account_id_from_email(&candidate.named)
                         .await
                         .caused_by(trc::location!())?;
-                    sent_by_id == Some(*sender_id)
+                    // Mutation20: no SENT-BY-to-changed_by check in the delegated branch.
+                    (true || sent_by_id == Some(*sender_id))
                         && named_id.is_some_and(|named_id| named_id != *sender_id)
                         && server.za_is_key_account(*sender_id).await?
                         && server.za_is_key_account(named_id.unwrap()).await?
                 }
             } else {
                 server
                     .za_account_id_from_email(&candidate.named)
```

## Mutation 21, attempt 1: inconclusive for assigned case

Required case: 9 (a delegated REQUEST carries its writer's SENT-BY, which carol's drain verifies against changed_by). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `7718e21ea7b694c9a844d317a079a50e6c9ba3464a2c3f27ac59e882d88602c5`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 100.519s. Decisive observation:
`sharing.rs:7329` (sharing_compare_lifecycle, case 25): `assertion left == right failed: HTTP delegated ORGANIZER must carry its writer's SENT-BY`, left `None`, right `Some("mailto:sharing-life-bob@example.com")`, at the first differential step (label `eleven`, sharing.rs:7766), run 1 (Bob's delegated write). Whole ZA failed 0 passed/1 failed in 45.29s (build within the 100.5s wall time); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: The mutated fan-out never calls za_mark_sent_by on its messages (fanout.rs:335), so neither the writer's SENT-BY is placed nor a client-supplied one stripped. Case 25 opens each pending item in Bob's own inbox; in the delegated run those are the second run's DelegatedSelf items, and their ALICE ORGANIZER carried no SENT-BY. Case 9's fixtures (sharing_http_schedule_contracts, reached from sharing_lifecycle_differential at sharing.rs:7876 after the differential steps) were not reached. The build's only new warning is the mutation's own `variable does not need to be mutable` (the closure's `mut message`).

Ruling: Inconclusive for case 9: the condition is detected, but by case 25's SENT-BY assertion on Bob's self items, an earlier fixture, not the assigned case. Case 9's oracle inspects first-run items delivered to another recipient (Carol), a distinct production path (Upstream provenance) from the self items case 25 inspects (DelegatedSelf provenance, the only messages Bob's inbox receives in a delegated run, since za_combine_delegated removes writer recipients from the first run), so attempt 2 narrows the same omission to Upstream-provenance messages. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `051cd8d1a4f518d593aa35b05654ba41cb8c03a372a7bccb4f44feea962b589d` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -328,15 +328,15 @@
         };
         // Reconciliation changes neither length nor order: zip the original
         // tags back onto precisely the same payload positions.
         let mut messages: Vec<_> = messages
             .into_iter()
             .zip(provenances)
             .map(|(mut message, provenance)| {
-                za_mark_sent_by(&mut message, writer_account, event_account);
+                // Mutation21 omits delegated SENT-BY placement.
                 ZaFanOutMessage {
                     message,
                     provenance,
                 }
             })
             .collect();
         let event = reconciled.as_ref().unwrap_or(event);
```

## Mutation 21, attempt 2: killed

Required case: 9 (a delegated REQUEST carries its writer's SENT-BY, which carol's drain verifies against changed_by). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `fc19fc5dd3636f3dc884725ed90a4bb4423da87e2e2c9efdd489d77167beef19`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 100.5s. Decisive observation:
`sharing.rs:8709` (sharing_http_schedule_contracts, case 9 `case9-delegated`): `assertion left == right failed`, left `[Some("mailto:sharing-life-dave@example.com")]`, right `[Some("mailto:sharing-life-bob@example.com")]`: the SENT-BY of the ALICE ORGANIZER in the REQUEST delivered to Carol. Whole ZA failed 0 passed/1 failed in 48.05s (build 44.86s); every case 25 differential step passed first (last printed `Case25 force-sole step1`).

State evidence: Bob PUTs `case9-delegated` into Alice's shared calendar with a client-supplied `ORGANIZER;SENT-BY="mailto:sharing-life-dave@example.com"`. Under the narrowed omission the first-run (Upstream-provenance) REQUEST to Carol skipped za_mark_sent_by, so the client's false SENT-BY was neither replaced nor stripped and Bob's was never placed; the item was delivered (1 item, changed_by Bob, both asserted before) carrying Dave as SENT-BY. Bob's DelegatedSelf items kept the original call, which is why case 25's SENT-BY assertion (attempt 1's failure) now passed. The fixture's next step, Carol's drain (release 2's sender check against changed_by Bob), was not reached; with SENT-BY naming Dave it would refuse the item, the spec's stated symptom.

Ruling: Killed at case 9 under the attempt-1 narrowing: case 9's own assertion that the delivered delegated REQUEST names its writer in SENT-BY fails, and the condition alone explains it. The narrowing keeps the attempt-1 code change identical on case 9's path (the call omitted for Upstream-provenance messages, every ordinary fan-out included, as in attempt 1, which passed all earlier modules) and leaves only DelegatedSelf messages on the original call, a production provenance split, not a fixture key. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `b8ceb46f20511df063405f73024ed14b08205462793b7c937119fce7d09f2c47` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -328,15 +328,18 @@
         };
         // Reconciliation changes neither length nor order: zip the original
         // tags back onto precisely the same payload positions.
         let mut messages: Vec<_> = messages
             .into_iter()
             .zip(provenances)
             .map(|(mut message, provenance)| {
-                za_mark_sent_by(&mut message, writer_account, event_account);
+                // Mutation21 omits SENT-BY placement on Upstream-provenance messages.
+                if provenance == ZaMessageProvenance::DelegatedSelf {
+                    za_mark_sent_by(&mut message, writer_account, event_account);
+                }
                 ZaFanOutMessage {
                     message,
                     provenance,
                 }
             })
             .collect();
         let event = reconciled.as_ref().unwrap_or(event);
```

## Mutation 22, attempt 1: inconclusive for assigned case

Required case: 10 (attendee token locality judged by the owner's addresses: Bob keeps his pair in Alice's event, so his later reply applies). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `ebb426258b15b564cb7478f3dfc6de62916e45f2128630153523f9c5008d9955`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 106.904s. Decisive observation:
`sharing.rs:7493` (sharing_compare_lifecycle, case 25): `assertion left == right failed: eleven step0: owner-stored token pair presence`, left `Some(Some((2, {"sharing-life-bob@example.com", "sharing-life-carol@example.com"})))` (owner run), right `Some(Some((1, {"sharing-life-carol@example.com"})))` (Bob's delegated run). Whole ZA failed 0 passed/1 failed in 44.83s (build 51.61s); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: At the first differential step (label `eleven`, step 0, a create), the delegated fan-out's token_union judged scheduled-attendee locality by the merged sender list (Bob's and Alice's addresses), so Bob counted as local, no pair was minted for him, and Alice's stored event kept Carol's pair only; the owner run of the same step stored pairs for Bob and Carol. The step's inbox-multiset assertion (sharing.rs:7489) passed before it. Case 10's fixtures (sharing_http_schedule_contracts, after the differential steps) were not reached.

Ruling: Inconclusive for case 10: the condition is detected, and exactly as the ledger describes (Bob's pair dropped from Alice's event), but by case 25's owner-stored pair-presence oracle at an earlier fixture. Row stopped here and reported to the coordinator without further attempts: case 25's step and case 10's fixtures take the same production path (a grantee's HTTP PUT as delegated organizer on Alice's shared `lifecycle` calendar, update.rs -> za_fan_out -> token_union, the organizer branch), and the owner run of case 25 is unaffected by construction (sender and event account coincide); no seam key separates them short of a fixture-specific condition (event UID or step), as row 13 attempt 1 found for its row. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `62a25633f4bca1858aaa00364bddcb98cdf0b93d77af57713a0a00ed11eafe33` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -343,15 +343,15 @@
         let organizer = event_organizer(event, sender.addresses());
         let is_organizer = organizer
             .as_ref()
             .is_some_and(|organizer| organizer.is_local);
         // Spec 3.3, "Maintenance": only the organizer hands out tokens; an
         // attendee's copy holds its own pair.
         let union = if is_organizer {
-            token_union(stored_pairs.as_ref(), event, event_account.addresses())
+            token_union(stored_pairs.as_ref(), event, sender.addresses())
         } else {
             stored_pairs.clone().unwrap_or_default()
         };

         // Phase 1: route every (message, address); resolve each attempted
         // address once, by its normalized form, with its served key; then
         // read the pin record.
```

## Mutation 23, attempt 1: inconclusive for assigned case

Required case: 10 (the writer's own inbox is admitted in a delegated fan-out: Bob's own copy advances, so his later reply is not stale). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `9f68f71c926261fbf4ca9b603460595ca7ad4a55511ae06769710863b00cab5e`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 99.865s. Decisive observation:
`sharing.rs:7489` (sharing_compare_lifecycle, case 25): `assertion left == right failed: eleven step0: inbox payload/address multisets`, left: one item in Bob's inbox (a REQUEST of the `create` step, attendees Bob and Carol) for the owner run, right `[]` for Bob's delegated run. Whole ZA failed 0 passed/1 failed in 45.37s (build 47.69s); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: At the first differential step (label `eleven`, step 0, a create), the delegated fan-out resolved Bob's own address with allow_self forced false, so resolve_inner answered NotInternal for the writer's account and no DelegatedSelf item reached Bob's inbox; the owner run delivered him one REQUEST. Case 10's fixtures (sharing_http_schedule_contracts, after the differential steps) were not reached.

Ruling: Inconclusive for case 10: the condition is detected (the writer's own inbox excluded from a delegated fan-out) by case 25's inbox-multiset oracle at an earlier fixture. Row stopped here and reported to the coordinator without further attempts: case 25's delegated create and case 10's fixtures take the same production path (a grantee's HTTP PUT as delegated organizer on Alice's shared `lifecycle` calendar, za_fan_out's self-admission at fanout.rs:422), and only a fixture-specific condition (event UID or step) would let case 10 run first. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `cd6ccd62c25cfdcbefa05c1151182a3cd25a585a5bbddef67f883902aa3b86c9` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -415,21 +415,15 @@
                 };
                 if let Some(key) = &key
                     && !recipients.contains_key(key)
                 {
                     let recipient = match resolved.take_if(|(cached, _)| cached == key) {
                         Some((_, recipient)) => recipient,
                         None => {
-                            let allow_self = resolution.is_some()
-                                && messages.iter().any(|candidate| {
-                                    candidate.provenance == ZaMessageProvenance::DelegatedSelf
-                                        && candidate.message.to.iter().any(|address| {
-                                            za_normalize_address(address).as_ref() == Some(key)
-                                        })
-                                });
+                            let allow_self = false;
                             resolve_recipient(
                                 self,
                                 sender_id,
                                 key,
                                 resolution.and_then(|resolution| resolution.accounts.get(key)),
                                 allow_self,
                                 resolution.map(|_| event_account.account_id),
```

## Mutation 24, attempt 1: inconclusive for assigned case

Required case: 10 aliases (a delegated fan-out delivers one self item per writer alias, two attempts on one account). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `c4a8c5ba69ffa39d9412585c393be1aeac6dbc8572cd40eca99ef945913c5c62`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 96.753s. Decisive observation:
`sharing.rs:7489` (sharing_compare_lifecycle, case 25): `assertion left == right failed: listed-alias step0: inbox payload/address multisets`; left (owner run) holds two REQUEST items in Bob's inbox, right (Bob's delegated run) one, each with attendees `sharing-life-bob`, `sharing-life-carol`, `sharing-life-work`. Whole ZA failed 0 passed/1 failed in 45.84s (build 41.19s); the `eleven` label's eleven steps passed first (printed `Case25 eleven step0..step10`).

State evidence: The draft was revised before its first execution: its collapse flag was reset per message, so two single-address DelegatedSelf messages (Bob on the master, his alias on an override, the fixtures' shape) would never collapse, and it dropped later alias attempts, which also removed their SCHEDULE-STATUS. The executed patch collapses only item writes: within one fan-out, the first delivered DelegatedSelf item to the writer's own account stands for every other address of that account, whose attempt keeps its route and records the delivered status without a write; same-address writes, other accounts' attempts and every Upstream-provenance attempt are unchanged. In `listed-alias` step 0 (Bob on the master, the alias on override rid1) the owner run delivered Bob two items, one per address; the delegated run wrote one, so the alias's item was missing. `eleven` (no alias attendee in its delegated self messages' distinct addresses) was unaffected. Case 10's alias fixture (`case10-alias-replies`, sharing.rs:9096-9121, in sharing_http_schedule_contracts after the differential) was not reached.

Ruling: Inconclusive for case 10 aliases: the condition is detected (one self item for all of the writer's aliases) by case 25's inbox-multiset oracle at its `listed-alias` fixture, an earlier fixture of the same shape. Row stopped here and reported to the coordinator without further attempts: `listed-alias` and `case10-alias-replies` take the same production path (Bob's HTTP PUT as delegated organizer on Alice's shared `lifecycle` calendar, with Bob on the master and an alias on an override, za_fan_out phase 3), so only a fixture-specific condition (event UID) would let case 10 run first. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `ea99d932dbd1133aa4fcc6a430281ac3c90420db635481916acf9aaedc90f49e` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -582,14 +582,16 @@
                 {
                     full_states.insert(key.as_str(), full);
                 }
             }
         }
         let mut statuses = Vec::new();
         let mut not_written: AHashSet<&str> = AHashSet::new();
+        // Mutation24: one self item for all of the writer's aliases.
+        let mut mutation_self_key: Option<&str> = None;
         for attempt in &attempts {
             let status = match &attempt.route {
                 Route::NotAttempted { .. } => ITIP_STATUS_NOT_ATTEMPTED,
                 Route::Deliver {
                     pair, reply_token, ..
                 } => match attempt.key.as_deref().map(|key| (key, recipients.get(key))) {
                     Some((
@@ -648,26 +650,39 @@
                                     record_status(
                                         &mut statuses,
                                         attempt.address,
                                         attempt.route.on_organizer(),
                                         status,
                                     );
                                 }
-                                let status = write_item(
-                                    self,
-                                    sender_id,
-                                    *account_id,
-                                    public_key,
-                                    &attempt.message.message,
-                                    pair.as_ref(),
-                                    reply_token.as_deref(),
-                                    fence,
-                                )
-                                .await
-                                .caused_by(trc::location!())?;
+                                let mutation_writer = attempt.self_allowed && *account_id == sender_id;
+                                let mutation_alias = mutation_writer
+                                    && mutation_self_key.is_some_and(|first| first != key);
+                                let status = if mutation_alias {
+                                    ITIP_STATUS_DELIVERED
+                                } else {
+                                    write_item(
+                                        self,
+                                        sender_id,
+                                        *account_id,
+                                        public_key,
+                                        &attempt.message.message,
+                                        pair.as_ref(),
+                                        reply_token.as_deref(),
+                                        fence,
+                                    )
+                                    .await
+                                    .caused_by(trc::location!())?
+                                };
+                                if mutation_writer
+                                    && mutation_self_key.is_none()
+                                    && status == ITIP_STATUS_DELIVERED
+                                {
+                                    mutation_self_key = Some(key);
+                                }
                                 if status != ITIP_STATUS_DELIVERED {
                                     not_written.insert(key);
                                 }
                                 status
                             }
                             Some(Verdict::Refuse) => ITIP_STATUS_KEY_REFUSED,
                             None => ITIP_STATUS_DELIVERY_FAILED,
```

## Mutation 22, attempt 2: killed

Required case: 10 (attendee token locality judged by the owner's addresses: Bob keeps his pair in Alice's event, so his later reply applies). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `26fe7c8fe96098d54872f02fc5887fa01658bceeac025eb1e29df4e78598290f`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 99.605s. Decisive observation:
`sharing.rs:9120` (sharing_http_schedule_contracts, case 10, fixture `case10-alias-replies`): `called Option::unwrap() on a None value` on `stored_pairs(test, &bob_keys, bob.id, own)`, the read of Bob's own copy's token pairs that precedes the fixture's pair-presence assertion (`sharing.rs:9121`, both Bob and his alias hold a pair) and its alias replies. Whole ZA failed 0 passed/1 failed in 51.04s (build 41.73s); last marker `Running case25 HTTP owner/delegated lifecycle differential...` (case 10 runs inside it, after case 25's steps).

State evidence: Scoped by event UID per ruling R11-22: the merged sender list replaces the event account's addresses in token_union only when `source.uid` starts with `case10-` (a prefix that occurs only in sharing_http_schedule_contracts: `case10-participation`, `case10-alias-replies`, `case10-sole-decline`, `case10-sole-delegated`, `case10-remove-self`; case 25's UIDs are `sharing-<label>-owner|delegated`). The observation-only trace printed every delegated organizer decision: all of case 25's (`sharing-eleven-delegated` x11, `sharing-listed-alias-delegated` x5, `sharing-subaddress-delegated` x7, `sharing-subaddress-only-delegated` x2, `sharing-force-sole-delegated` x2) and case 9's (`case9-delegated` x3, `case9-cancel-gain`, `case9-cancel-loss`, `case9-per-component`, `case9-own-organizer`) scoped=false, and case 25 and case 9 passed; only `case10-participation` (Bob's DECLINED write) and `case10-alias-replies` (Bob's delegated create) scoped=true. `case10-participation` passed under the condition: token_union keeps every stored pair and only mints for scheduled attendees without one, so the pair Alice's own create minted for Bob survives Bob's later write and his reply applies. At `case10-alias-replies`, Bob's delegated create has no stored pairs; the merged list classified Bob and his alias as local, so no pair was minted for either, and Bob's own copy, imported from his self items, holds no token pairs at all (stored_pairs None), where the original holds pairs for both aliases.

Ruling: Killed at case 10: the run failed in case 10's own alias-replies fixture, at the read of Bob's own copy's token pairs, with the state the condition explains (no pair minted for a writer-local attendee, so Bob has no reply token and his later replies cannot apply). The decisive point is the token-carrier precondition of Bob's reply, one step before the reply itself; the participation fixture, whose pairs were minted by Alice's own create, is insensitive to the condition by construction. Attempt 1 (unscoped) remains recorded as detection by case 25. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `6853701bdeaf04d8bedc2311909f22174ede59e0b683696d30a40d499106999c` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -343,15 +343,31 @@
         let organizer = event_organizer(event, sender.addresses());
         let is_organizer = organizer
             .as_ref()
             .is_some_and(|organizer| organizer.is_local);
         // Spec 3.3, "Maintenance": only the organizer hands out tokens; an
         // attendee's copy holds its own pair.
         let union = if is_organizer {
-            token_union(stored_pairs.as_ref(), event, event_account.addresses())
+            // Mutation22 (R11-22): locality by the merged sender list, case 10's UIDs only.
+            let mutation22_scoped = source.uid.starts_with("case10-");
+            if delegated {
+                eprintln!(
+                    "Mutation22 delegated token locality: uid={}, scoped={}",
+                    source.uid, mutation22_scoped
+                );
+            }
+            token_union(
+                stored_pairs.as_ref(),
+                event,
+                if mutation22_scoped {
+                    sender.addresses()
+                } else {
+                    event_account.addresses()
+                },
+            )
         } else {
             stored_pairs.clone().unwrap_or_default()
         };

         // Phase 1: route every (message, address); resolve each attempted
         // address once, by its normalized form, with its served key; then
         // read the pin record.
```

## Mutation 23, attempt 2: killed

Required case: 10 (the writer's own inbox is admitted in a delegated fan-out: Bob's own copy advances, so his later reply is not stale). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `842ebd0b0eefab7743fb02c2e6b212a63f0347768201ca2db4b1cd9a9033fb65`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 102.426s. Decisive observation:
`sharing.rs:9022` (sharing_http_schedule_contracts, case 10, fixture `case10-participation`): `assertion left == right failed`, left `Some("ACCEPTED")`, right `Some("DECLINED")`: after Bob set his own PARTSTAT to DECLINED through Alice's shared event and the drain steps ran, his own copy still read ACCEPTED; the next assertion (sharing.rs:9026, his own copy's SEQUENCE equal to Alice's event) was not reached. Whole ZA failed 0 passed/1 failed in 52.52s (build 42.30s); last marker `Running case25 HTTP owner/delegated lifecycle differential...` (case 10 runs inside it, after case 25's steps).

State evidence: Scoped by event UID per ruling R11-22: the original self admission (resolution present and a DelegatedSelf message addressed to the key) is computed unchanged and then forced false only when `source.uid` starts with `case10-` (a prefix that occurs only in sharing_http_schedule_contracts). The observation-only trace printed every admission the original would grant: 36 unscoped (`sharing-eleven-delegated` 11, `sharing-listed-alias-delegated` 8, `sharing-subaddress-delegated` 13, `sharing-subaddress-only-delegated` 2, `sharing-force-sole-delegated` 2; case 25 passed) and one scoped, `case10-participation` (Bob's DECLINED write as delegated organizer). Case 9's fixtures granted no self admission and passed. Under the scoped condition resolve_recipient refused Bob's own account for that fan-out, so no DelegatedSelf item reached his inbox and his own copy kept the previous state (ACCEPTED), where the original delivers him the update that the drain applies.

Ruling: Killed at case 10: the run failed at case 10's own-copy assertion with exactly the state the condition explains (the writer's own inbox excluded from the delegated fan-out, so Bob's own copy does not advance and his later reply would be judged against a stale copy). Attempt 1 (unscoped) remains recorded as detection by case 25. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `daa9c1a0a2d6201a461974b91b715d47bb3e24b94d4a3b6283861a6de17c95e5` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -422,14 +422,23 @@
                             let allow_self = resolution.is_some()
                                 && messages.iter().any(|candidate| {
                                     candidate.provenance == ZaMessageProvenance::DelegatedSelf
                                         && candidate.message.to.iter().any(|address| {
                                             za_normalize_address(address).as_ref() == Some(key)
                                         })
                                 });
+                            // Mutation23 (R11-22): writer's own inbox excluded, case 10's UIDs only.
+                            let mutation23_scoped = source.uid.starts_with("case10-");
+                            if allow_self {
+                                eprintln!(
+                                    "Mutation23 self admission: uid={}, scoped={}",
+                                    source.uid, mutation23_scoped
+                                );
+                            }
+                            let allow_self = allow_self && !mutation23_scoped;
                             resolve_recipient(
                                 self,
                                 sender_id,
                                 key,
                                 resolution.and_then(|resolution| resolution.accounts.get(key)),
                                 allow_self,
                                 resolution.map(|_| event_account.account_id),
```

## Mutation 24, attempt 2: killed

Required case: 10 aliases (a delegated fan-out delivers one self item per writer alias, two attempts on one account). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `094d2758ed457622d8fe246cb6bc9db86ee4aafb68029087982b50819cdbc415`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 105.606s. Decisive observation:
`sharing.rs:9121` (sharing_http_schedule_contracts, case 10, fixture `case10-alias-replies`): `assertion failed: pairs.get(LIFE_BOB).is_some() && pairs.get(LIFE_ALIAS).is_some()`: after Bob's delegated create (Bob on the master, his alias on override 20990302T100000Z) and his drain, his own copy did not hold a token pair for both addresses. Whole ZA failed 0 passed/1 failed in 49.23s (build 49.22s); last marker `Running case25 HTTP owner/delegated lifecycle differential...` (case 10 runs inside it, after case 25's steps).

State evidence: Scoped by event UID per ruling R11-22: the attempt-1 collapse (the first delivered DelegatedSelf item to the writer's own account stands for every other address of that account, whose attempt records the delivered status without a write) applies only when `source.uid` starts with `case10-` (a prefix that occurs only in sharing_http_schedule_contracts). The observation-only trace printed every second writer address the unscoped collapse would skip: 9 unscoped (`sharing-listed-alias-delegated` 3, `sharing-subaddress-delegated` 6; case 25 passed, including `listed-alias`, which detected attempt 1) and one scoped, `case10-alias-replies` (Bob's create). Under the scoped condition the alias's self item, its token carrier, was not written, so Bob's own copy imported one item and lacks one of the two address pairs; the original writes one item per address and the copy holds both.

Ruling: Killed at case 10 aliases: the run failed at case 10's alias fixture with exactly the state the condition explains (one self item for both of the writer's addresses, so one alias has no token carrier and could not reply). The decisive assertion is the fixture's per-alias token-carrier check (the live-preparation map's `token carrier` oracle for this row), the first case-10 observation of the missing item, ahead of its alias replies. Attempt 1 (unscoped) remains recorded as detection by case 25's inbox multiset. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `302263b28eda3ecef96f7e7b5b96035bbe95a0bd1bfdb513f1faf24316644b68` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -582,14 +582,16 @@
                 {
                     full_states.insert(key.as_str(), full);
                 }
             }
         }
         let mut statuses = Vec::new();
         let mut not_written: AHashSet<&str> = AHashSet::new();
+        // Mutation24: one self item for all of the writer's aliases.
+        let mut mutation_self_key: Option<&str> = None;
         for attempt in &attempts {
             let status = match &attempt.route {
                 Route::NotAttempted { .. } => ITIP_STATUS_NOT_ATTEMPTED,
                 Route::Deliver {
                     pair, reply_token, ..
                 } => match attempt.key.as_deref().map(|key| (key, recipients.get(key))) {
                     Some((
@@ -648,26 +650,48 @@
                                     record_status(
                                         &mut statuses,
                                         attempt.address,
                                         attempt.route.on_organizer(),
                                         status,
                                     );
                                 }
-                                let status = write_item(
-                                    self,
-                                    sender_id,
-                                    *account_id,
-                                    public_key,
-                                    &attempt.message.message,
-                                    pair.as_ref(),
-                                    reply_token.as_deref(),
-                                    fence,
-                                )
-                                .await
-                                .caused_by(trc::location!())?;
+                                let mutation_writer = attempt.self_allowed && *account_id == sender_id;
+                                let mutation_candidate = mutation_writer
+                                    && mutation_self_key.is_some_and(|first| first != key);
+                                // Mutation24 (R11-22): case 10's UIDs only.
+                                let mutation_scoped = source.uid.starts_with("case10-");
+                                if mutation_candidate {
+                                    eprintln!(
+                                        "Mutation24 second writer address: uid={}, scoped={}",
+                                        source.uid, mutation_scoped
+                                    );
+                                }
+                                let mutation_alias = mutation_candidate && mutation_scoped;
+                                let status = if mutation_alias {
+                                    ITIP_STATUS_DELIVERED
+                                } else {
+                                    write_item(
+                                        self,
+                                        sender_id,
+                                        *account_id,
+                                        public_key,
+                                        &attempt.message.message,
+                                        pair.as_ref(),
+                                        reply_token.as_deref(),
+                                        fence,
+                                    )
+                                    .await
+                                    .caused_by(trc::location!())?
+                                };
+                                if mutation_writer
+                                    && mutation_self_key.is_none()
+                                    && status == ITIP_STATUS_DELIVERED
+                                {
+                                    mutation_self_key = Some(key);
+                                }
                                 if status != ITIP_STATUS_DELIVERED {
                                     not_written.insert(key);
                                 }
                                 status
                             }
                             Some(Verdict::Refuse) => ITIP_STATUS_KEY_REFUSED,
                             None => ITIP_STATUS_DELIVERY_FAILED,
```

## Mutation 25, attempt 1: killed

Required case: 12 (no single-parent rule: a MOVE of an event linked into two unshared calendars into a shared calendar is refused 403, its archive unchanged). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `7d8bfe73286c7ab7f60bd487807c83093406b7c20e4057bc1caff974fdc254a2`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 123.69s. Decisive observation:
`sharing.rs:747` via `tests/src/utils/webdav.rs:599` (copy_move_test, case 12): `Expected 403 Forbidden but got 201 Created` for `MOVE {HOME}a/linked.ics` to `{HOME}c/linked.ics` (an event COPY-linked into the unshared calendars a and b, moved into the shared calendar c); the following archive-version assertion (sharing.rs:748-751) was not reached. Whole ZA failed 0 passed/1 failed in 67.84s (build 48.90s); last marker `Running conversion assertion, rollback, resume and cache contracts...`, after `Task5 legacy event id=2` (the single-name legacy MOVE into c in the same fixture passed).

State evidence: The draft (za_move_contexts' two `names.len() != 1` refusals) was audited and extended before execution to the whole rule on the MOVE path, M6-style: rewrap_event also refuses a rewrap into a shared domain of an event with more than one name (seal Structure "collection key event parent"), which under the draft alone would have refused case 12's linked MOVE with a seal error instead of a clean 201. The executed patch removes the shared-source and shared-target sole-parent refusals in za_move_contexts and routes za_copy_move_stored_event (alone, by an import alias) to a temporary copy of rewrap_event without its target sole-parent rule; conversion and every other caller keep the original rewrap_event, the other-parent shared refusal in za_move_contexts is unchanged, and the copy's legacy-plaintext branch keeps its inner check (the linked row is EWK-sealed and never reaches it). Observation-only traces fired exactly once each, at the linked MOVE: `Mutation25 sole-parent rule skipped at the shared target: names=2` and `Mutation25 rewrap target sole-parent rule skipped: names=2`. The MOVE then rewrapped the event's DEK to the shared calendar's CK, kept its b name beside the new c name, and committed: 201.

Ruling: Killed at case 12: the linked MOVE that the single-parent rule must refuse with 403 succeeded (201), the state the condition explains (an event of a shared calendar left with a second parent). Source restored to its exact pre-mutation bytes (both files).

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/copy_move.rs` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` | `49c960b1430e18a0cebef053392c92e628e3383d3df95ac44f2734369799013a` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` |
| `crates/groupware/src/calendar/seal/event.rs` | `abd1cfa6bfcc97a23d3620ed2e8d00d03f8d0450c128044acf13a7543bb8e5af` | `325313c651b2909629d60743acac60b5ab176441723215ea65d3baebc561ff61` | `abd1cfa6bfcc97a23d3620ed2e8d00d03f8d0450c128044acf13a7543bb8e5af` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/copy_move.rs
+++ b/crates/dav/src/calendar/copy_move.rs
@@ -1276,15 +1276,15 @@
         return Err(DavError::Code(StatusCode::NOT_FOUND));
     }
     if source_context.domain.calendar_id().is_some() {
         if !is_move || is_collection {
             return Err(shared_copy_refusal(owner));
         }
         if event.names.len() != 1 {
-            return Err(linked_move_refusal(owner));
+            eprintln!("Mutation25 sole-parent rule skipped at the shared source: names={}", event.names.len());
         }
     }
     contexts.push(source_context);
     for parent in event
         .names
         .iter()
         .map(|name| name.parent_id)
@@ -1315,15 +1315,15 @@
         .map_err(|error| super::za_calendar_error(error, owner, target))?
         .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
     if target_context.domain.calendar_id().is_some() {
         if !is_move || is_collection {
             return Err(shared_copy_refusal(owner));
         }
         if event.names.len() != 1 {
-            return Err(linked_move_refusal(owner));
+            eprintln!("Mutation25 sole-parent rule skipped at the shared target: names={}", event.names.len());
         }
     }
     // Keep a separate target entry even for a same-calendar rename: its operation
     // bits were freshly checked and callers have a stable destination position.
     contexts.push(target_context);
     Ok(contexts)
 }
@@ -1456,15 +1456,15 @@
     vanished: Option<String>,
     schedule_tag: Option<u32>,
     is_move: bool,
     is_collection: bool,
     original: Archive<AlignedBytes>,
 ) -> crate::Result<HttpResponse> {
     use groupware::calendar::{
-        seal::{classify_event_storage, rewrap_event, seal_error},
+        seal::{classify_event_storage, event::rewrap_event_mutation25 as rewrap_event, seal_error},
         sharing::resolve::calendars_changed,
     };
     let event = original.deserialize::<CalendarEvent>()?;
     if schedule_tag.is_some() && event.schedule_tag != schedule_tag {
         return Err(DavError::Code(StatusCode::PRECONDITION_FAILED));
     }
     let mut contexts = za_move_contexts(
--- a/crates/groupware/src/calendar/seal/event.rs
+++ b/crates/groupware/src/calendar/seal/event.rs
@@ -562,14 +562,53 @@
 ) -> Result<(), SealError> {
     if from.owner_id() != to.owner_id() {
         return Err(SealError::Structure("cross-account event rewrap"));
     }
     check_domain_parent(event, from)?;
     if to.calendar_id().is_some() && event.names.len() != 1 {
         return Err(SealError::Structure("collection key event parent"));
+    }
+    if classify_event_storage(event)? == ZaEventStorageKind::LegacyPlaintext {
+        // MOVE may still name the source calendar while sealing for the target.
+        let mut changed = event.clone();
+        let names = changed.names.clone();
+        if let Some(calendar) = to.calendar_id() {
+            if changed.names.len() != 1 {
+                return Err(SealError::Structure("collection key event parent"));
+            }
+            changed.names[0].parent_id = calendar;
+        }
+        seal_event_in_domain(&mut changed, to, None)?;
+        changed.names = names;
+        *event = changed;
+        return Ok(());
+    }
+    let uid = uid_of(event);
+    let key = event.data.event.components[0].entries.last_mut().unwrap();
+    let envelope = STANDARD
+        .decode(entry_text(key).ok_or(SealError::Format)?)
+        .map_err(|_| SealError::Decode)?;
+    let dek = from.open_event_key(&envelope, &uid)?;
+    key.values[0] =
+        calcard::icalendar::ICalendarValue::Text(STANDARD.encode(to.seal_event_key(&dek, &uid)?));
+    Ok(())
+}
+
+/// Mutation25: MOVE-only copy without the target sole-parent rule.
+pub fn rewrap_event_mutation25(
+    event: &mut CalendarEvent,
+    from: &ZaKeyDomain,
+    to: &ZaKeyDomain,
+) -> Result<(), SealError> {
+    if from.owner_id() != to.owner_id() {
+        return Err(SealError::Structure("cross-account event rewrap"));
+    }
+    check_domain_parent(event, from)?;
+    if to.calendar_id().is_some() && event.names.len() != 1 {
+        eprintln!("Mutation25 rewrap target sole-parent rule skipped: names={}", event.names.len());
     }
     if classify_event_storage(event)? == ZaEventStorageKind::LegacyPlaintext {
         // MOVE may still name the source calendar while sealing for the target.
         let mut changed = event.clone();
         let names = changed.names.clone();
         if let Some(calendar) = to.calendar_id() {
             if changed.names.len() != 1 {
```

## Mutation 26, attempt 1: inconclusive for assigned case

Required case: 12 (a successful unshared-to-shared MOVE leaves the event with the target as its sole parent, CK envelope, readable by owner and Bob). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `2600b02245e27a840dbbf13e290faafa02d2c730e48f9f728ea3247f24d450aa`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 100.096s. Decisive observation:
`tests/src/utils/webdav.rs:599` from `set_partstat` (sharing.rs:8894) in sharing_http_schedule_contracts, case 9 fixture `case9-attendee-reply`: `Expected 200 OK but got 500 Internal Server Error` for Bob's GET of `{LIFE_PATH}case9-attendee-reply.ics`, after Alice's MOVE of her imported copy from her unshared default calendar into her shared `lifecycle` calendar answered 201. Whole ZA failed 0 passed/1 failed in 53.09s (build 39.68s); last marker `Running case25 HTTP owner/delegated lifecycle differential...` (case 9 runs inside it, after case 25's steps). Case 12 (copy_move_test) runs later and was not reached.

State evidence: The draft kept the source name on every key-account MOVE; it was narrowed before execution to the ledger's condition, a MOVE whose target domain is shared (every other MOVE removes its source name as before). The observation-only trace fired once, at case 9's MOVE (`Mutation26 MOVE into a shared calendar keeps the source name: names before push=1`): the event was rewrapped to the shared calendar's CK and stored with two names (default and lifecycle). The next read of it through the shared calendar failed in the seal layer's read-parentage check (a CK row must have exactly its calendar as its sole parent), a 500. The shared-shape guard the live-preparation note anticipated is this read-side check; it did not refuse the MOVE itself.

Ruling: Inconclusive for case 12: the condition is detected (an event of a shared calendar left with a second name becomes unreadable), but in case 9's fixture and as a generic 500, before case 12 runs. Case 9's MOVE and case 12's take the same production path (za_copy_move_stored_event, an owner's MOVE from an unshared calendar into a shared one), so no seam key separates them short of a fixture-specific condition; row stopped and reported to the coordinator with a proposed scope (case 12's event UIDs, which all start with `move-`, as ruling R11-22 scoped rows 22-24). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/copy_move.rs` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` | `93b7033ea31a8a2b8c402c7606c5b8224e8bfb04588bdabb7dcced9ad4d67bb4` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/copy_move.rs
+++ b/crates/dav/src/calendar/copy_move.rs
@@ -1529,15 +1529,23 @@
         }
         if is_move {
             let index = changed
                 .names
                 .iter()
                 .position(|name| name.parent_id == source)
                 .unwrap();
-            changed.names.swap_remove(index);
+            // Mutation26: a MOVE into a shared calendar keeps the event's other names.
+            if to.calendar_id().is_some() {
+                eprintln!(
+                    "Mutation26 MOVE into a shared calendar keeps the source name: names before push={}",
+                    changed.names.len()
+                );
+            } else {
+                changed.names.swap_remove(index);
+            }
         }
         changed.names.push(DavName {
             name: new_name.to_owned(),
             parent_id: target,
         });
         let mut batch = BatchBuilder::new();
         for context in &contexts {
```

## Mutation 27, attempt 1: killed

Required case: 14 race (owner registry destruction during a parked authoritative grant: 409 account deleted, no orphan ACL index under a remaining grantee). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `33611729a0c7359c5fac9b50ef08d96a02d0de8f39e42b2f4e5bd5616010a458`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 197.852s. Decisive observation:
`sharing_races.rs:3414` via `tests/src/utils/za.rs:145` (sharing_destruction_contracts, case 14): `assertion left == right failed: unexpected status`, left `401` (body `You have to authenticate first.`), right `409`, for the grant parked at the share commit pause while the owner's registry Account was destroyed; the `account deleted` check and the orphan-ACL assertions after it were not reached. Whole ZA failed 0 passed/1 failed in 138.37s (build 50.76s); last marker `Running HTTP sharing fences, conversion budgets and resume tests...`.

State evidence: The draft was audited: only za_share_apply's batch calls the temporary assert_in_without_account (vault and pin assertions kept); the conversion batches keep ZaShareGuards::assert_in with the Account assertion. The observation-only trace showed the parked grant's batch assembled without the Account assertion (the sixth `owner=49` line, the destruction fixture's owner). Causal chain from the code path: on an assertion loss the endpoint re-runs `guards`, whose first check answers 409 `account deleted` once the registry Account is gone (shares.rs:110-120), and the only steps after the pause are the commit and, on success, the boundary completion, whose account lookup fails for a destroyed account with an Auth error (401). A 401 therefore means the commit was not refused. Attempt 2 records this directly.

Ruling: Killed at case 14: the parked grant was not refused by the registry Account assertion (none was in its batch), so the owner's destruction did not stop it and the case's 409 `account deleted` oracle failed. Source restored to its exact pre-mutation bytes (both files).

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `0b96dba42a1179888c168cb205ce24769873555761b010d435a305d31f9390fb` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `7ef84b60af9f6b39c6725899648a63c5b6dc55a5396c8f05c6c1ce2c3724a0f0` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -104,14 +104,31 @@
             ZaCalendarError::Store(error) => Self::Store(error),
         }
     }
 }

 impl ZaShareGuards {
     /// Append every externally verified fence before any indexed write.
+    pub fn assert_in_without_account(&self, batch: &mut BatchBuilder) {
+        batch
+            .with_account_id(self.owner_id)
+            .with_collection(Collection::Principal)
+            .with_document(0)
+            .assert_value(
+                PrincipalField::ZeroAccessVault,
+                AssertValue::Hash(self.vault_cas),
+            );
+        if let Some(pin) = self.pin_assertion {
+            batch.assert_value(PrincipalField::ZeroAccessPins, pin);
+        }
+        eprintln!(
+            "Mutation27 share batch without the registry Account assertion: owner={}",
+            self.owner_id
+        );
+    }
     pub fn assert_in(&self, batch: &mut BatchBuilder) {
         batch
             .with_account_id(self.owner_id)
             .with_collection(Collection::Principal)
             .with_document(0)
             .assert_value(
                 PrincipalField::ZeroAccessVault,
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -427,15 +427,15 @@
     za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut batch = BatchBuilder::new();
     context.assert_in(&mut batch);
-    guards.assert_in(&mut batch);
+    guards.assert_in_without_account(&mut batch);
     calendar.update(
         changed_by,
         context.stored.to_unarchived::<Calendar>()?,
         keys.account_id,
         calendar_id,
         &mut batch,
     )?;
```

## Mutation 27, attempt 2: killed

Required case: 14 race (owner registry destruction during a parked authoritative grant: 409 account deleted, no orphan ACL index under a remaining grantee). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `d80b603e43d38db33e703ffbc37fddafc83f7ef1dd32b8ef661a9a9f0ae20941`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 182.275s. Decisive observation:
`sharing_races.rs:3414` via `tests/src/utils/za.rs:145` (sharing_destruction_contracts, case 14): `assertion left == right failed: unexpected status`, left `401` (body `You have to authenticate first.`), right `409`. Whole ZA failed 0 passed/1 failed in 113.33s (build 1m 01s); last marker `Running HTTP sharing fences, conversion budgets and resume tests...`.

State evidence: Same condition as attempt 1, plus observation-only traces of the commit outcome (mutations.rs) and of the post-commit boundary completion's error (shares.rs). For the destruction fixture's owner (49) the trace shows the parked grant's batch without the Account assertion followed by `Mutation27 share batch commit: owner=49, new_grant=Some(38), committed=true` (38 is the second grantee, granted after the owner's registry destruction), then `Mutation27 boundary completion after the commit failed: Store(... Auth(Error) ... Details "Account not found." ... AccountId 49 ...)`, which the endpoint returned as 401. So the grant's ACL and ring write committed for an owner whose registry Account no longer existed. (An earlier fixture's boundary completion failure in the HTTP fence races, `Seal(Structure("unshared calendar entries"))` for owner 48, matched that fixture's expectation; the suite continued past it.)

Ruling: Killed at case 14, with direct evidence: the share batch without the registry Account assertion committed the parked grant after the owner's destruction (committed=true for new_grant 38), the state the condition explains, and the case's 409 `account deleted` oracle failed. Confirms attempt 1's causal reading. Source restored to its exact pre-mutation bytes (all three files).

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `0b96dba42a1179888c168cb205ce24769873555761b010d435a305d31f9390fb` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `4d0b6a1d283b32bc2dffcb530bbfa21eaa59036f774860d92d598fce25e577d3` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |
| `crates/http/src/api/shares.rs` | `8ea709cb248858d020207a489f7608dc18e557b1e4b0eb6fbb7f6926e43f596f` | `90fa56b78af5b69e5f9020d8f0b098fbc032e91682d052446d3ec5e0d147c23f` | `8ea709cb248858d020207a489f7608dc18e557b1e4b0eb6fbb7f6926e43f596f` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -104,14 +104,31 @@
             ZaCalendarError::Store(error) => Self::Store(error),
         }
     }
 }

 impl ZaShareGuards {
     /// Append every externally verified fence before any indexed write.
+    pub fn assert_in_without_account(&self, batch: &mut BatchBuilder) {
+        batch
+            .with_account_id(self.owner_id)
+            .with_collection(Collection::Principal)
+            .with_document(0)
+            .assert_value(
+                PrincipalField::ZeroAccessVault,
+                AssertValue::Hash(self.vault_cas),
+            );
+        if let Some(pin) = self.pin_assertion {
+            batch.assert_value(PrincipalField::ZeroAccessPins, pin);
+        }
+        eprintln!(
+            "Mutation27 share batch without the registry Account assertion: owner={}",
+            self.owner_id
+        );
+    }
     pub fn assert_in(&self, batch: &mut BatchBuilder) {
         batch
             .with_account_id(self.owner_id)
             .with_collection(Collection::Principal)
             .with_document(0)
             .assert_value(
                 PrincipalField::ZeroAccessVault,
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -427,25 +427,32 @@
     za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut batch = BatchBuilder::new();
     context.assert_in(&mut batch);
-    guards.assert_in(&mut batch);
+    guards.assert_in_without_account(&mut batch);
     calendar.update(
         changed_by,
         context.stored.to_unarchived::<Calendar>()?,
         keys.account_id,
         calendar_id,
         &mut batch,
     )?;
     #[cfg(feature = "test_mode")]
     common::za::test_hooks::share_commit_pause_point(keys.account_id).await;
-    let committed = server.commit_batch(batch).await?;
+    let committed = server.commit_batch(batch).await;
+    eprintln!(
+        "Mutation27 share batch commit: owner={}, new_grant={:?}, committed={}",
+        keys.account_id,
+        new_grant,
+        committed.is_ok()
+    );
+    let committed = committed?;
     server.notify_task_queue();
     if new_grant.is_some() {
         applied.boundary_commit = Some(ZaBoundaryCommit {
             change_id: committed.last_change_id(keys.account_id)?,
         });
     }
     applied.affected_grantees.sort_unstable();
--- a/crates/http/src/api/shares.rs
+++ b/crates/http/src/api/shares.rs
@@ -346,15 +346,18 @@
                 for grantee in applied.affected_grantees {
                     invalidation.invalidate(CacheInvalidation::AccessToken(grantee));
                 }
                 server.invalidate_caches(invalidation).await?;
                 if let Some(commit) = applied.boundary_commit {
                     za_complete_boundary(server, &verified.keys, id, Some(commit))
                         .await
-                        .map_err(|e| e.into_store_error(verified.account_id, id))?;
+                        .map_err(|e| {
+                            eprintln!("Mutation27 boundary completion after the commit failed: {e:?}");
+                            e.into_store_error(verified.account_id, id)
+                        })?;
                 }
                 trc::event!(
                     Store(trc::StoreEvent::DataWrite),
                     AccountId = verified.account_id,
                     DocumentId = id,
                     Total = counts.rewrapped,
                     Size = counts.sealed,
```

## Mutation 28, attempt 1: killed

Required case: 5 alias transfer (an address the ledger records for one grantee and the directory now resolves to another is ambiguous: 409, ring unchanged). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `8d7aa5df9dbe5f6e3d9689c0c4f92f15518f2cdcc71a70d5379a199de82e1d47`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 139.683s. Decisive observation:
`sharing.rs:3173` via `tests/src/utils/za.rs:145` (sharing_rotation_contracts, case 5): `assertion left == right failed: unexpected status, body: {"ok":true,"changed":true}`, left `200`, right `409`, for `shares/revoke` by the address `sharing-alias` after it moved from the reader (whose grant records it) to the second grantee; the `address is ambiguous, revoke by grant` and unchanged-generations assertions after it were not reached. Whole ZA failed 0 passed/1 failed in 67.01s (build 1m 02s); last marker `Running transactional sharing endpoint tests...`.

State evidence: The draft was audited and corrected before execution: it returned no grant when the directory resolved the address to a non-grantee (never consulting the ledger) and replaced the ledger's own ambiguity set by a first match. The executed patch consults the directory first: a directory answer naming a current grantee wins outright; otherwise the original ledger lookup, with its ambiguity refusal, runs. The observation-only trace fired twice: the earlier revoke by the reader's primary address (`grant=34, ledger rows with the address=0`, the same outcome as the original), then the alias revoke (`grant=38, ledger rows with the address=1`): the directory's answer (the second grantee, 38) was taken although the ledger records the alias for the reader, so the second grantee's grant was revoked (changed true) instead of the request being refused as ambiguous.

Ruling: Killed at case 5: the revoke resolved the transferred alias through the directory before the ledger and removed the wrong grantee, the state the condition explains, failing the case's 409 ambiguity oracle. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `abd722120ebed1be6b7e9083c0223a109a883f786e566c519df64890e2a1cb61` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -234,31 +234,41 @@
     {
         return Err(ZaShareError::TooManyShares);
     }
     let revoke = if let ZaShareChange::Revoke { target } = &change {
         match target {
             ZaRevokeTarget::Grant(id) => Some(*id),
             ZaRevokeTarget::Address(address) => {
-                let mut candidates = ring
-                    .grants
-                    .iter()
-                    .filter(|g| g.address == *address)
-                    .map(|g| g.account_id)
-                    .collect::<BTreeSet<_>>();
+                // Mutation28: the directory before the ledger.
                 if let Some(id) = server
                     .za_account_id_from_email(address)
                     .await?
                     .filter(|id| ring.grants.iter().any(|g| g.account_id == *id))
                 {
-                    candidates.insert(id);
+                    let ledger = ring
+                        .grants
+                        .iter()
+                        .filter(|g| g.address == *address)
+                        .count();
+                    eprintln!(
+                        "Mutation28 revoke by address resolved through the directory first: grant={id}, ledger rows with the address={ledger}"
+                    );
+                    Some(id)
+                } else {
+                    let candidates = ring
+                        .grants
+                        .iter()
+                        .filter(|g| g.address == *address)
+                        .map(|g| g.account_id)
+                        .collect::<BTreeSet<_>>();
+                    if candidates.len() > 1 {
+                        return Err(ZaShareError::AmbiguousAddress);
+                    }
+                    candidates.first().copied()
                 }
-                if candidates.len() > 1 {
-                    return Err(ZaShareError::AmbiguousAddress);
-                }
-                candidates.first().copied()
             }
         }
     } else {
         None
     };
     let mut removed = Vec::new();
     for grant in &ring.grants {
```

## Mutation 29, attempt 1: survived

Required case: 1 (an incremental sync of a granted calendar emits its own tombstone and never counts or emits a tombstone of an ungranted calendar, result limit 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `5072e6473235f0871cdfdf0552f8209c2500ff2e9380be92b31bd6e75d52d04c`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 500.176s. Decisive observation:
Case 1's tombstone assertions passed under the condition: `sharing.rs:6976-6989` (grantee_sync_boundaries, for both the home and the shared targets with `Some(1)` result limit) found `{shared}deleted.ics` and no `/private/` href. The run later failed in an unrelated module: `tests/src/utils/za.rs:145` `unexpected status, body: {"error":"account already holds mail"}`, left 409, right 200, in the mail provisioning fence tests (`setup_refuses_spam_samples` the last printed sub-test, host load average about 90). Whole ZA failed 0 passed/1 failed in 376.09s (build 1m 40s); every sharing, sharing-race and leak module passed before it.

State evidence: The draft (the single vanished-path filter of a foreign key account's sync-collection) was audited and executed as drafted, plus an observation-only trace. The trace shows the condition took effect at case 1's two limit-1 syncs: `Mutation29 vanished paths unfiltered: total=2, outside granted prefixes=1` (the private calendar's tombstone kept beside the granted one), and `total=0` at the later after-rename sync. The private tombstone was not emitted because the fixture deletes the granted event before the private one: the vanished list is in change order, so with `total_changes` 2 above the limit 1 the truncation (propfind.rs:1623-1660) emits the granted tombstone first and drops the private one into a continuation (`is_sync_limited`, a `seq+1` sync token). The counted private tombstone is therefore unobservable through the fixture's href checks.

Ruling: Survived: unresolved coverage, not an equivalence (an unfiltered private tombstone is counted against the limit and would be emitted on the continuation or without a limit). Missing oracle and proposed test-only correction at the existing seam: in the tombstone block of grantee_sync_boundaries (sharing.rs:6957-6990), delete the private calendar's event before the granted one (loop order `["private", "shared"]`), so a counted private tombstone takes the single limit-1 slot and the existing `missing granted tombstone` assertion fails; and add the same sync without a result limit (`None`) requiring no `/private/` href and, for the limit-1 request, that the response's sync token carries no continuation (not sync-limited). No production change. Source restored to its exact pre-mutation bytes; row 29 to be re-run after the oracle wave.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `0e7162bad066825fdec36113d7a691c462236ba4696371f6cb1535c9f961c91c` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1461,15 +1461,24 @@
                     "{}{}/",
                     resources.base_path,
                     percent_encoding::utf8_percent_encode(&calendar.name, groupware::RFC_3986)
                 ));
             }
         }
         if let Some(vanished) = &mut foreign_vanished {
-            vanished.retain(|path| prefixes.iter().any(|prefix| path.starts_with(prefix)));
+            // Mutation29 leaves vanished paths unfiltered.
+            let mutation29_dropped = vanished
+                .iter()
+                .filter(|path| !prefixes.iter().any(|prefix| path.starts_with(prefix.as_str())))
+                .count();
+            eprintln!(
+                "Mutation29 vanished paths unfiltered: total={}, outside granted prefixes={}",
+                vanished.len(),
+                mutation29_dropped
+            );
         }
         // Reuse precisely these freshly read archives for the rest of this response.
         for (&id, context) in eligible {
             data.calendar_memo.insert((account_id, id), context.clone());
         }
     }

```

## Mutation 30, attempt 1: killed

Required case: 1 slug reuse (a token taken before a private calendar at a reused slug was deleted answers 403 valid-sync-token once the new calendar is granted; the full sync shows nothing of the old one). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `879e563a669ab32a24b65f5790248aae5044a67a60a8d9190e188465b4c28ad8`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 111.913s. Decisive observation:
`sharing.rs:8527` via `tests/src/utils/webdav.rs:599` (grantee_pending_grant_and_slug_reuse, case 1 slug reuse): `Expected 403 Forbidden but got 207 Multi-Status` for Bob's sync-collection of `reused-grant/` from the token taken before Alice deleted her private calendar at that slug, recreated it and granted it; the 207 body lists the old calendar's tombstone (`reused-grant/` 404) and the new calendar. Whole ZA failed 0 passed/1 failed in 60.70s (build 41.23s); last marker `Running Task 10 grantee DAV admission and method matrix...`.

State evidence: The draft (the numeric `id <= max(grant, rename)` bound of the incremental-sync token check, pending refusal kept) was audited and executed as drafted, plus an observation-only trace of each refusal the bound would have made. The trace fired twice: first `calendar=2, token=0, bound=7` during the pending-grant block's home-wide sync (the original refuses on that granted calendar first; under the mutation the loop continued to the pending calendar, which still refused, so the request answered 403 either way), then `calendar=6, token=19, bound=25` at the slug-reuse sync: the token's change id 19 is at or below the new calendar's grant boundary 25, which the original refuses, and the mutation served the sync.

Ruling: Killed at case 1 slug reuse: the old token was admitted past the grant boundary and the response disclosed the former calendar's history (its tombstone at the reused slug), the state the condition explains. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `a8ac95f8f80a8c2a854037f393a8b5cdf2124a4b5dc0a09ce765300a8a15b0ec` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1434,16 +1434,24 @@
         {
             return Err(DavError::Code(StatusCode::FORBIDDEN));
         }
         if let SyncType::From { id, .. } = query.sync_type {
             for (&calendar_id, context) in eligible {
                 if addressed_id.is_none_or(|id| id == calendar_id)
                     && context.boundary.is_some_and(|boundary| {
+                        // Mutation30: the token bound check removed.
+                        if !boundary.pending
+                            && id <= boundary.grant_change_id.max(boundary.rename_change_id)
+                        {
+                            eprintln!(
+                                "Mutation30 token bound skipped: calendar={calendar_id}, token={id}, bound={}",
+                                boundary.grant_change_id.max(boundary.rename_change_id)
+                            );
+                        }
                         boundary.pending
-                            || id <= boundary.grant_change_id.max(boundary.rename_change_id)
                     })
                 {
                     return Err(DavErrorCondition::new(
                         StatusCode::FORBIDDEN,
                         BaseCondition::ValidSyncToken,
                     )
                     .into());
```

## Mutation 31, attempt 1: inconclusive for assigned case

Required case: 1 rename into a former slug (a token kept from before a granted calendar was renamed into a slug that held a deleted private calendar answers 403 valid-sync-token; the full sync shows nothing of the old calendar). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `d142dead866cb1c3a45b2e44b6326182d980b0b958d69cede134336ed2928201`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 110.103s. Decisive observation:
`sharing.rs:7055` (grantee_sync_boundaries, case 1, paused-rename fixture): `called Result::unwrap() on an Err value: Elapsed(())`, the fixture's 5 s wait (sharing.rs:7050-7055) for the rename's boundary completion to reach `SHARE_BOUNDARY_PAUSE`. The fixture's rename-into-a-former-slug oracles after it (the numeric pre-rename token and the kept token `later` answering 403 valid-sync-token, sharing.rs:7083-7100, and the full sync without the private history) were not reached. Whole ZA failed 0 passed/1 failed in 54.36s (build 43.59s); last marker `Running Task 10 sync boundary, rename and private tombstone controls...`.

State evidence: The draft (rename_container neither marks nor completes the shared calendar's boundary; the rename's own write, vanished log entry and rename pause point unchanged) was audited and executed as drafted, plus an observation-only trace, which fired once, at the fixture's paused rename (`Mutation31 shared calendar rename leaves its boundary unchanged: calendar=2`). With no completion step, the boundary pause the fixture waits for can never arrive, so the fixture stops at its hook wait. This holds for every implementation of the condition: za_complete_boundary returns before its pause point when the boundary is not pending, so a rename that does not advance the boundary never reaches it.

Ruling: Inconclusive for case 1's rename-into-a-former-slug oracle: the failure is the fixture's deterministic hook-arrival wait, caused by the condition (no boundary step), not a host-load client timeout, but it precedes the row's decisive token assertions, and the brief counts timeouts as inconclusive. For the coordinator: either accept the hook wait as case 1's own oracle for the rename's two-step boundary (spec case 1: SHARE_BOUNDARY_PAUSE holds the request between the steps), or add the missing oracle, test-only at existing seams: an unpaused rename into a former slug as spec case 1 describes it (Alice grants a calendar at `/other/`, Bob syncs and keeps the token, Alice creates and deletes an event in a private `/family/` and deletes that calendar, renames `/other/` to `/family/` with no hook set; Bob's kept token must answer 403 valid-sync-token and his full sync must show no private tombstone), placed before the paused rename; under this condition the kept token would be admitted and the private calendar's tombstone, now under the granted prefix, emitted. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/copy_move.rs` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` | `6675185de4e78dfbd00b149ca3cfee999ef65fa5993651ff67f63f3a0c26faad` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/copy_move.rs
+++ b/crates/dav/src/calendar/copy_move.rs
@@ -1183,14 +1183,17 @@
         .deserialize::<Calendar>()
         .caused_by(trc::location!())?;
     new_calendar.name = new_name.to_string();
     let shared = context
         .as_ref()
         .is_some_and(|context| context.domain.calendar_id().is_some());
     if shared {
+        eprintln!("Mutation31 shared calendar rename leaves its boundary unchanged: calendar={document_id}");
+    }
+    if false && shared {
         groupware::calendar::sharing::boundary::mark_boundary_pending(
             &mut new_calendar,
             &context.as_ref().unwrap().domain,
         )
         .map_err(|error| groupware::calendar::seal::seal_error(error, account_id, document_id))?;
     }

@@ -1207,15 +1210,15 @@
     batch.log_vanished_item(VanishedCollection::Calendar, from_resource_path);
     #[cfg(feature = "test_mode")]
     common::za::test_hooks::share_rename_pause_point(account_id).await;
     let assigned = server
         .commit_batch(batch)
         .await
         .caused_by(trc::location!())?;
-    if shared {
+    if false && shared {
         groupware::calendar::sharing::boundary::za_complete_boundary(
             server,
             access_token
                 .session_keys()
                 .ok_or(DavError::Code(StatusCode::FORBIDDEN))?,
             document_id,
             Some(groupware::calendar::sharing::boundary::ZaBoundaryCommit {
```

## Mutation 32, attempt 1: killed

Required case: 1 paused rename (private history committed while the rename is held before its commit stays behind the boundary: after completion the rename boundary exceeds the last private change id, and a numeric token above every pre-rename id is refused). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `0a192e5b1f559f51e8a5bb24e96ae0fa4e3a90fd6999794b0ca6fc348e5881f3`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 106.238s. Decisive observation:
`sharing.rs:7084` (grantee_sync_boundaries, case 1, paused rename): `assertion failed: renamed.boundary.unwrap().rename_change_id > private_history`, after the parked rename completed (201) and its boundary step was released. Whole ZA failed 0 passed/1 failed in 45.88s (build 52.89s, with the temporary mutant's `unused variable: assigned` warning in dav); last marker `Running Task 10 sync boundary, rename and private tombstone controls...`.

State evidence: The draft only passed the pre-commit change id as the rename's own-commit hint. Audited before execution: za_complete_boundary uses that hint only when the archive it rereads carries exactly that change id, which is never true for an id read before the rename's own write, and otherwise rereads the account log after the commit, so the draft alone would leave the boundary correct (equivalent by construction, not the condition). The executed patch reads the account's last Calendar change id before the rename pause point and commit, passes it as the hint, and routes the rename alone to a temporary copy of za_complete_boundary whose first attempt takes the boundary from the hint as given; the grant endpoint and the owner repair keep the original. The observation-only trace confirms both the condition and the audit: `Mutation32 rename boundary from the pre-commit id: id=33, archive change id=Some(37), identity check would match=false`. The boundary was completed at 33, below the private calendar's history (created, written and deleted at the vacant destination while the rename was parked) and below the rename's own commit 37.

Ruling: Killed at case 1 paused rename: the boundary taken from the change id read before the rename's commit sits below the private history committed in the pause, the state the condition explains, failing the case's boundary assertion (the numeric-token refusal after it was not reached). Source restored to its exact pre-mutation bytes (both files).

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/copy_move.rs` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` | `a2170a02a078d614696b8bc3d3ccd88099c469d25721d41d39fa920f861eb318` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` |
| `crates/groupware/src/calendar/sharing/boundary.rs` | `30e7be9cc4f9f06a052a200d091bc8fb5c73ff1dedb37b2d75f835d9117d30f4` | `50b7516a9c63971f021ea0eee009cee94c1880370e487ee8e8a6a80e708adc02` | `30e7be9cc4f9f06a052a200d091bc8fb5c73ff1dedb37b2d75f835d9117d30f4` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/copy_move.rs
+++ b/crates/dav/src/calendar/copy_move.rs
@@ -1201,29 +1201,30 @@
             calendar,
             account_id,
             document_id,
             &mut batch,
         )
         .caused_by(trc::location!())?;
     batch.log_vanished_item(VanishedCollection::Calendar, from_resource_path);
+    let mutation_before_commit = server.store().get_last_change_id(account_id, types::collection::SyncCollection::Calendar.into()).await?.unwrap_or_default();
     #[cfg(feature = "test_mode")]
     common::za::test_hooks::share_rename_pause_point(account_id).await;
     let assigned = server
         .commit_batch(batch)
         .await
         .caused_by(trc::location!())?;
     if shared {
-        groupware::calendar::sharing::boundary::za_complete_boundary(
+        groupware::calendar::sharing::boundary::za_complete_boundary_mutation32(
             server,
             access_token
                 .session_keys()
                 .ok_or(DavError::Code(StatusCode::FORBIDDEN))?,
             document_id,
             Some(groupware::calendar::sharing::boundary::ZaBoundaryCommit {
-                change_id: assigned.last_change_id(account_id)?,
+                change_id: mutation_before_commit,
             }),
         )
         .await
         .map_err(|error| super::za_calendar_error(error, account_id, document_id))?;
     }
     server.notify_task_queue();

--- a/crates/groupware/src/calendar/sharing/boundary.rs
+++ b/crates/groupware/src/calendar/sharing/boundary.rs
@@ -194,14 +194,78 @@
             }
             Err(error) => return Err(error.into()),
         }
     }
     unreachable!()
 }

+/// Mutation32: rename-only copy. On its first CAS loss,
+/// reread the archive *then* the account log and make one conservative fallback attempt.
+pub async fn za_complete_boundary_mutation32(
+    server: &Server,
+    keys: &SessionKeys,
+    calendar_id: u32,
+    own_commit: Option<ZaBoundaryCommit>,
+) -> Result<Archive<AlignedBytes>, ZaCalendarError> {
+    let attempts = if own_commit.is_some() { 2 } else { 1 };
+    for attempt in 0..attempts {
+        let context = za_owned_calendar_context(server, keys, calendar_id).await?;
+        if !context.boundary.is_some_and(|b| b.pending) {
+            return Ok(context.stored);
+        }
+        // Mutation32: the boundary taken from the id the rename read before its commit.
+        if attempt == 0
+            && let Some(own) = own_commit.as_ref()
+        {
+            eprintln!(
+                "Mutation32 rename boundary from the pre-commit id: id={}, archive change id={:?}, identity check would match={}",
+                own.change_id,
+                context.stored.version.change_id(),
+                context.stored.version.change_id() == Some(own.change_id)
+            );
+        }
+        let id = if attempt == 0 && own_commit.is_some() {
+            own_commit.as_ref().unwrap().change_id
+        } else {
+            server
+                .store()
+                .get_last_change_id(keys.account_id, SyncCollection::Calendar.into())
+                .await?
+                .unwrap_or_default()
+        };
+        let mut calendar = context.stored.deserialize::<Calendar>()?;
+        complete_boundary(&mut calendar, &context.domain, id)?;
+        let account = server.account_info(keys.account_id).await?;
+        let mut batch = BatchBuilder::new();
+        calendar.update(
+            account.account_tenant_ids(),
+            context.stored.to_unarchived::<Calendar>()?,
+            keys.account_id,
+            calendar_id,
+            &mut batch,
+        )?;
+        #[cfg(feature = "test_mode")]
+        common::za::test_hooks::share_boundary_pause_point(keys.account_id).await;
+        match server.commit_batch(batch).await {
+            Ok(_) => {
+                return Ok(za_owned_calendar_context(server, keys, calendar_id)
+                    .await?
+                    .stored);
+            }
+            Err(error) if error.is_assertion_failure() => {
+                if attempt + 1 == attempts {
+                    return Ok(context.stored);
+                }
+            }
+            Err(error) => return Err(error.into()),
+        }
+    }
+    unreachable!()
+}
+
 /// Explicit adapter for already authorized contexts. This never manufactures another
 /// owner's keys, and resolves the repair-returned archive before a caller uses its fence.
 pub async fn za_repair_owned_context(
     server: &Server,
     keys: &SessionKeys,
     context: ZaCalendarContext,
 ) -> Result<ZaCalendarContext, ZaCalendarError> {
```

## Mutation 33, attempt 1: inconclusive for assigned case

Required case: 1 paused rename (while the rename's boundary is pending, incremental syncs answer 403 valid-sync-token and the initial sync, no token, is served). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `f02e2c8c47938b4c72be084a4b4c4aff907830d7512cde5fe45744a7cb2536be`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 154.067s. Decisive observation:
`sharing.rs:8490` via `tests/src/utils/webdav.rs:599` (grantee_pending_grant_and_slug_reuse, the pending-grant block before slug reuse): `Expected 207 Multi-Status but got 403 Forbidden` (body `valid-sync-token`) for Bob's initial sync (no token) of `pending-grant/` while its grant was parked at `SHARE_BOUNDARY_PAUSE` with the boundary pending. Whole ZA failed 0 passed/1 failed in 60.64s (build 1m 13s); last marker `Running Task 10 grantee DAV admission and method matrix...`. grantee_sync_boundaries (the planted pending boundary, then the paused rename) runs after it and was not reached.

State evidence: The draft was audited and corrected before execution: the ledger condition is that an initial sync is refused while the boundary is pending (the opposite of the protection that serves it); the draft refused every non-incremental request (SyncType::None included, so plain PROPFINDs and other REPORTs) for any pending eligible calendar, regardless of the addressed one. The executed patch extends the incremental branch's own pending refusal, with its addressed-calendar selection, to SyncType::Initial only. The observation-only trace fired once (`Mutation33 initial sync refused while pending: calendar=4`, the pending-grant calendar), at the first initial sync of a pending boundary in the suite.

Ruling: Inconclusive for case 1 paused rename: the condition is detected, exactly as the protection it removes (an initial sync of a pending calendar refused), but at the pending-grant fixture, which precedes both the planted pending boundary (sharing.rs:6926) and the paused rename (sharing.rs:7068) on the same production path (a foreign key calendar's sync-collection with no token while its boundary is pending). A grant's and a rename's pending boundary differ in the grantee's header (grant change id still zero for a pending grant), but the planted block before the paused rename has the rename's shape, so only a fixture-specific scope (the paused rename's destination calendar, current name `reused`) reaches the paused rename first. Row stopped and reported to the coordinator: accept the pending-grant detection as case 1's (it is the same rule and the same oracle form), or approve the fixture scope. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `382dceccd0ffb93ad54229be7137ebf2b9250830c05aa229ae7c0399d6be29a2` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1430,14 +1430,29 @@
                     item.parent_id().unwrap()
                 }
             });
         if resource.resource.is_some() && addressed_id.is_some_and(|id| !eligible.contains_key(&id))
         {
             return Err(DavError::Code(StatusCode::FORBIDDEN));
         }
+        // Mutation33: an initial sync refused while the boundary is pending.
+        if matches!(query.sync_type, SyncType::Initial) {
+            for (&calendar_id, context) in eligible {
+                if addressed_id.is_none_or(|id| id == calendar_id)
+                    && context.boundary.is_some_and(|boundary| boundary.pending)
+                {
+                    eprintln!("Mutation33 initial sync refused while pending: calendar={calendar_id}");
+                    return Err(DavErrorCondition::new(
+                        StatusCode::FORBIDDEN,
+                        BaseCondition::ValidSyncToken,
+                    )
+                    .into());
+                }
+            }
+        }
         if let SyncType::From { id, .. } = query.sync_type {
             for (&calendar_id, context) in eligible {
                 if addressed_id.is_none_or(|id| id == calendar_id)
                     && context.boundary.is_some_and(|boundary| {
                         boundary.pending
                             || id <= boundary.grant_change_id.max(boundary.rename_change_id)
                     })
```

## Mutation 26, attempt 2: killed

Required case: 12 (a successful unshared-to-shared MOVE leaves the event with the target as its sole parent, CK envelope, readable by owner and Bob). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `1ea6a69f36a636fa371dd1a8eb05da21a12085080755017ae711306571c73a2a`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 115.47s. Decisive observation:
`tests/src/za/sharing.rs:599` (copy_move_test, case 12's first unshared-to-shared MOVE, `a/sealed.ics` into the shared `c/sealed.ics`, UID `move-sealed`): `assertion left == right failed`, left `2`, right `1` for `moved.names.len()`, after the MOVE answered the required 201 (sharing.rs:594). Whole ZA failed 0 passed/1 failed in 68.91s (build 38.44s); last marker `Running conversion assertion, rollback, resume and cache contracts...` (case 24's fixtures; copy_move_test, which follows them, prints no marker before this point).

State evidence: The observation-only trace fired twice in the whole run: at case 9's MOVE into its shared lifecycle calendar (`Mutation26 MOVE into a shared calendar: uid=Some("case9-attendee-reply"), scoped=false, names before removal=1`: the original source-name removal kept, and case 9 passed this time) and at case 12's first MOVE into a shared calendar (`uid=Some("move-sealed"), scoped=true, names before removal=1`). The scoped MOVE committed with both names (the source `a` name kept, the shared target `c` name added) and answered 201; the archive read straight from the store then held two names, the condition itself (an event in a shared calendar keeping another parent). Before preparing, every `move-` string in tests/src was mapped: only copy_move_test (sharing.rs:571-854: move-sealed, move-legacy, move-linked, move-bad, move-overwritten, move-overwrite-source, move-bob-overwritten, move-bob-source) uses the prefix as an event UID; no earlier za module or fixture does.

Ruling: Killed at case 12: the required observation (the moved event's sole parent is the shared target) fails exactly as the condition explains, after the required 201. Scope per ruling R11-26 (the R11-22 precedent): the physical omission is attempt 1's (za_copy_move_stored_event skips the source-name removal for a MOVE whose target domain is shared), keyed additionally on case 12's event UIDs (prefix `move-`), so case 9's MOVE, which detected attempt 1 first as a read-side 500, keeps the original rule; every other MOVE is unchanged, and the trace shows the only scoped decision was case 12's. Attempt 1's detection at case 9 stays recorded as additional evidence. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/copy_move.rs` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` | `1eafb2a1af84f7e946607edc8aeeba430604f1b2af366c091cf0126974894b84` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/copy_move.rs
+++ b/crates/dav/src/calendar/copy_move.rs
@@ -1529,15 +1529,29 @@
         }
         if is_move {
             let index = changed
                 .names
                 .iter()
                 .position(|name| name.parent_id == source)
                 .unwrap();
-            changed.names.swap_remove(index);
+            // Mutation26 (R11-26): a MOVE into a shared calendar keeps the event's other names,
+            // case 12's UIDs only.
+            let mutation26_uid = event.data.event.uids().next();
+            let mutation26_scoped = mutation26_uid.is_some_and(|uid| uid.starts_with("move-"));
+            if to.calendar_id().is_some() {
+                eprintln!(
+                    "Mutation26 MOVE into a shared calendar: uid={:?}, scoped={}, names before removal={}",
+                    mutation26_uid,
+                    mutation26_scoped,
+                    changed.names.len()
+                );
+            }
+            if !(to.calendar_id().is_some() && mutation26_scoped) {
+                changed.names.swap_remove(index);
+            }
         }
         changed.names.push(DavName {
             name: new_name.to_owned(),
             parent_id: target,
         });
         let mut batch = BatchBuilder::new();
         for context in &contexts {
```

## Mutation 33, attempt 2: killed

Required case: 1 paused rename (while the rename's boundary is pending, incremental syncs answer 403 valid-sync-token and the initial sync, no token, is served). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `cfb75811d054671e79ad9ea84182b914940304043a5f3cf039509a427be1592b`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 96.661s. Decisive observation:
`sharing.rs:7068` via `tests/src/utils/webdav.rs:599` (grantee_sync_boundaries, the paused rename, first loop target Bob's home while the rename's boundary completion is held at `SHARE_BOUNDARY_PAUSE`): `Expected 207 Multi-Status but got 403 Forbidden`, body `<D:valid-sync-token/>`, for Bob's initial sync (no token). The same iteration's incremental sync with token `fffffffffffffffe` had answered the required 403 just before. Whole ZA failed 0 passed/1 failed in 46.58s (build 42.86s); last marker `Running Task 10 sync boundary, rename and private tombstone controls...`.

State evidence: The observation-only trace printed every initial-sync decision on a pending eligible calendar: twice in the pending-grant fixture (`calendar=4, name=pending-grant, scoped=false`, its pending_path and home syncs, both served 207 this time), twice in the planted rename-shaped pending block at sharing.rs:6926 (`calendar=2, name=shared, scoped=false`, its shared and home syncs, both served), then once at the paused rename (`calendar=2, name=reused, scoped=true`), the shared calendar after rename_container wrote its new name `reused` with the boundary pending, refused with 403 valid-sync-token. Before preparing, every use of the name was mapped: the exact calendar name `reused` occurs only in grantee_sync_boundaries' paused rename (sharing.rs:7005-7123: the rename's destination, and the private calendar Alice creates and deletes there while the rename is held, never granted to Bob and so never eligible); the pending-grant fixture uses `pending-grant` and `reused-grant` (excluded by the exact match), and the planted block's calendar is still named `shared`.

Ruling: Killed at case 1 paused rename: the required observation (the initial sync is served while the rename's boundary is pending) fails exactly as the condition explains, with the protection's own refusal form, while the paused rename's incremental refusal still holds. Scope per ruling R11-33: the physical change is attempt 1's (the incremental branch's pending refusal, with its addressed-calendar selection, extended to SyncType::Initial), keyed additionally on the pending calendar's current name `reused`, so the pending-grant fixture and the planted block, which detected attempt 1 first, keep the original rule; the trace shows the only scoped decision was the paused rename's. Attempt 1's detection in case 1's pending-grant sub-case stays recorded. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `57e83e591a17fbb2e55883d4b8f7ed3aff54e29edaea64c708ade142d463bbfc` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1430,14 +1430,40 @@
                     item.parent_id().unwrap()
                 }
             });
         if resource.resource.is_some() && addressed_id.is_some_and(|id| !eligible.contains_key(&id))
         {
             return Err(DavError::Code(StatusCode::FORBIDDEN));
         }
+        // Mutation33 (R11-33): an initial sync refused while the boundary is pending, for the
+        // paused rename's destination calendar (`reused`) only.
+        if matches!(query.sync_type, SyncType::Initial) {
+            for (&calendar_id, context) in eligible {
+                if addressed_id.is_none_or(|id| id == calendar_id)
+                    && context.boundary.is_some_and(|boundary| boundary.pending)
+                {
+                    let mutation33_name = context
+                        .stored
+                        .deserialize::<Calendar>()
+                        .map(|calendar| calendar.name)
+                        .unwrap_or_default();
+                    let mutation33_scoped = mutation33_name == "reused";
+                    eprintln!(
+                        "Mutation33 initial sync while pending: calendar={calendar_id}, name={mutation33_name}, scoped={mutation33_scoped}"
+                    );
+                    if mutation33_scoped {
+                        return Err(DavErrorCondition::new(
+                            StatusCode::FORBIDDEN,
+                            BaseCondition::ValidSyncToken,
+                        )
+                        .into());
+                    }
+                }
+            }
+        }
         if let SyncType::From { id, .. } = query.sync_type {
             for (&calendar_id, context) in eligible {
                 if addressed_id.is_none_or(|id| id == calendar_id)
                     && context.boundary.is_some_and(|boundary| {
                         boundary.pending
                             || id <= boundary.grant_change_id.max(boundary.rename_change_id)
                     })
```

## Mutation 34, attempt 1: killed

Required case: 24 first run (phase 2's complete post-phase-1 parentage walk precedes every CK rewrap: an earlier sole-parent EWK row stays unchanged, with its account envelope, when a later row is found linked and the revert write fails). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `2f2ab450f91e2801acc770c3b05f14881b6b52303a44ea1f716136dbd67b9178`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 128.585s. Decisive observation:
`tests/src/za/sharing.rs:1438` (conversion_contract_tests, the Mutation34 oracle on calendar `walk-before-write`): `assertion left == right failed: mutation34: earlier unlinked EWK candidate changed before failed revert`, left `Versioned { change_id: 8, hash: 3257001268 }`, right `Versioned { change_id: 4, hash: 1512579773 }` (the early row's archive version after the conversion returned, against its version before it started). Whole ZA failed 0 passed/1 failed in 66.07s (build 55.09s); last marker `Running conversion assertion, rollback, resume and cache contracts...`.

State evidence: The fixture planted two sole-parent EWK rows in `walk-before-write` (early, a lower document id, before late), paused the conversion at the precheck point after phase 1's clean walk, linked late into `other`, armed one injected failure of the Revert write, and released. The observation-only trace fired once (`Mutation34 linked row acted on after the CK rewrap of event 1: calendar=3`): phase 2's walk still ran first and returned has_link, but the mutant acted on it only after the first eligible row's rewrap; early's CK rewrap was committed (its archive version moved from change 4 to change 8), then the Revert write met the injected failure and the conversion returned, before rollback could rewrap early back. The oracle observes that state at the failed revert. Only the Mutation34 oracle's conversion_link and precheck pause precede this point in the suite (the first post-phase-1 link in it), so no earlier fixture reached the delayed decision.

Ruling: Killed at case 24's first run: the required observation (no CK rewrap before the complete post-phase-1 parentage walk has decided; the earlier sole-parent EWK row unchanged at the failed revert) fails exactly as the condition explains. The draft was audited against the real control flow before execution: the has_link block (mark_reverting, the Revert write, wrote, continue) moves verbatim to just after the first eligible row's Rewrap commit inside the chunk loop, with a labelled continue so its next step is the original's (re-read, reverting journal, rollback, LinkedEvents); event_ids still runs at the top, and the per-row parent refusal, rollback, final validate_scan and every guard are unchanged. Where no eligible row precedes the linked one, the mutant meets the per-row parent refusal instead; case 24's fixture asserts early < late, so it exercises the intended path. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/convert.rs` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` | `8010d0490d7a057b79d2a98b355ee0b098b6433c5b02404f13d1e676859985b3` | `5d2938f682431bbf57325bdb6983b4aeaae70157dd30dd1ddc1ee569de8455df` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/convert.rs
+++ b/crates/groupware/src/calendar/sharing/convert.rs
@@ -165,15 +165,15 @@
     }
     let changed_by = server
         .account_info(keys.account_id)
         .await?
         .account_tenant_ids();
     let mut counts = ZaConversionCounts::default();
     let mut linked = false;
-    loop {
+    'mutation_conversion: loop {
         let context = za_owned_calendar_context(server, keys, calendar_id).await?;
         let ring = owner_ring(&context, keys)?;
         if let Some((_, journal)) = &ring
             && (!journal.converted || journal.reverting)
         {
             let calendar = context.stored.deserialize::<Calendar>()?;
             if !journal.grants.is_empty()
@@ -221,31 +221,14 @@
                 }
             }
             Some((_header, ring)) if ring.converted => return Ok(counts),
             Some((_header, _ring)) => {
                 // This complete post-phase-1 walk closes the unfenced precheck
                 // window before even the first EWK envelope is changed.
                 let (ids, has_link) = event_ids(server, keys.account_id, calendar_id).await?;
-                if has_link {
-                    linked = true;
-                    let calendar = mark_reverting(&context, keys)?;
-                    write_calendar(
-                        server,
-                        keys,
-                        guards,
-                        changed_by,
-                        &context,
-                        calendar,
-                        WriteClass::Revert,
-                        counts,
-                    )
-                    .await?;
-                    *wrote = true;
-                    continue;
-                }
                 for (index, ids) in ids.chunks(100).enumerate() {
                     let chunk = read_chunk(server, keys.account_id, ids).await?;
                     if index > 0 {
                         conversion_cut(keys.account_id).await?;
                     }
                     for (id, stored) in &chunk {
                         let mut event = stored.deserialize::<CalendarEvent>()?;
@@ -273,14 +256,32 @@
                             &stored,
                             event,
                             WriteClass::Rewrap,
                             counts,
                         )
                         .await?;
                         *wrote = true;
+                if has_link {
+                    eprintln!("Mutation34 linked row acted on after the CK rewrap of event {id}: calendar={calendar_id}");
+                    linked = true;
+                    let calendar = mark_reverting(&context, keys)?;
+                    write_calendar(
+                        server,
+                        keys,
+                        guards,
+                        changed_by,
+                        &context,
+                        calendar,
+                        WriteClass::Revert,
+                        counts,
+                    )
+                    .await?;
+                    *wrote = true;
+                    continue 'mutation_conversion;
+                }
                         match kind {
                             ZaEventStorageKind::LegacyPlaintext => counts.sealed += 1,
                             ZaEventStorageKind::Sealed => counts.rewrapped += 1,
                         }
                     }
                 }
                 // No grant-ready result until every current row has sole parent
```

## Mutation 35, attempt 1: killed

Required case: 24 second run (a cut conversion journal with a raced-in EWK link refuses PUT and PROPPATCH through both names with 409 while the journal is shared, keeping both links and the stored data). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `65bc474a115fb48c6919bb98c64e5326b2c5cf78778f661efc9706b34d7669c1`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 138.25s. Decisive observation:
`sharing.rs:2031` via `tests/src/utils/webdav.rs:599` (conversion_contract_tests, the linked-cut fixture, first name `linked-cut/ewk.ics`): `Expected 409 Conflict but got 204 No Content` for the owner's PUT, after that name's GET answered the required 200. Whole ZA failed 0 passed/1 failed in 67.53s (build 1m 00s); last marker `Running conversion assertion, rollback, resume and cache contracts...`, after the `Mutation34 oracle: ...` line (case 24's first run passed unmutated).

State evidence: The fixture left `linked-cut` as a cut journal (Init written, then the injected interruption at the conversion cut) holding the planted EWK row with two names (linked-cut and other). Both observation-only traces fired once, for that PUT: `Mutation35 linked-shared shortcut skipped: owner=41, addressed=19, names=2` (the addressed shared calendar's shortcut that answers 409 first) and `Mutation35 linked write rule skipped: owner=41, contexts=2` (check_event_domains' rule over the shared and the unshared parent). With neither refusal, the PUT passed the resolver and answered 204 at upstream's unchanged-content shortcut (crates/dav/src/calendar/update.rs:238, `ical == view`), since the request body equals the planted event; by code reading (not observed in this run) a changed body through this name would next meet the seal layer's own parent check (check_domain_parent under the CK domain) as a 500, and through the `other` name, whose first context is the owner's EWK domain, would be sealed and written. The PROPPATCH, the second name and the unlink/resume steps were not reached.

Ruling: Killed at case 24's second run: the required 409 for a write through a name of a linked event while the journal is shared is absent exactly because the condition removed both resolver copies of the linked write rule (the original answers 409 from za_write_contexts before any content comparison); the 204 is the request admitted. The patch is the ledger's two-site draft, audited before execution and given observation-only traces; check_event_domains is shared with za_owned_event_contexts (drain, contact endpoints), as drafted, and no earlier fixture reached either removed rule (the traces fired only here). The seal layer's parent checks were left in place. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/resolve.rs` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` | `c44fd2db9713551fb2ed228e07e097ddf863d7ed8af508ebaaba011eb6e25146` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/resolve.rs
+++ b/crates/groupware/src/calendar/sharing/resolve.rs
@@ -466,15 +466,22 @@
     let mut contexts = Vec::with_capacity(parents.len());
     // Authorize the requested path first. An authorized shared path plus
     // another stored name is already a linked-write refusal; opening a
     // private other parent must not replace that 409 with an unrelated 403.
     if let Some(context) =
         za_calendar_context(server, token, owner_id, addressed_calendar, required).await?
     {
-        let linked_shared = event.names.len() > 1 && context.domain.calendar_id().is_some();
+        // Mutation35: the linked-shared write shortcut removed.
+        if event.names.len() > 1 && context.domain.calendar_id().is_some() {
+            eprintln!(
+                "Mutation35 linked-shared shortcut skipped: owner={owner_id}, addressed={addressed_calendar}, names={}",
+                event.names.len()
+            );
+        }
+        let linked_shared = false;
         contexts.push(context);
         if linked_shared {
             return check_event_domains(contexts, true);
         }
     }
     for parent in parents
         .into_iter()
@@ -514,18 +521,28 @@
     }
     Ok(parents.into_iter().collect())
 }
 fn check_event_domains(
     contexts: Vec<ZaCalendarContext>,
     linked: bool,
 ) -> Result<Vec<ZaCalendarContext>, ZaCalendarError> {
-    if linked
+    // Mutation35: the linked write rule removed.
+    let mutation35_rule = linked
         && contexts
             .iter()
-            .any(|context| context.domain.calendar_id().is_some())
+            .any(|context| context.domain.calendar_id().is_some());
+    if mutation35_rule {
+        eprintln!(
+            "Mutation35 linked write rule skipped: owner={}, contexts={}",
+            contexts[0].owner_id,
+            contexts.len()
+        );
+    }
+    let mutation35_apply = false;
+    if mutation35_apply && mutation35_rule
     {
         trc::event!(
             Security(trc::SecurityEvent::Unauthorized),
             AccountId = contexts[0].owner_id,
             Details = "zero-access: event linked into a shared calendar"
         );
         Err(ZaCalendarError::LinkedEvent)
```

## Mutation 36, attempt 1: killed

Required case: 24 third run (a session's collection-key cache keeps a key of an earlier conversion apart from the current one: the cache identity includes the key-domain id); attempt 1 runs the direct unit oracle vault::session::tests::collection_cache_separates_reconversions. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `63103d1404ca7013cdfcc8ea1712dfa4d414c2770e9a12aa36875b0c4b9e0eff`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test -p vault collection_cache_separates_reconversions -- --nocapture
```

Exit `101`, elapsed wall time 23.72s. Decisive observation:
`crates/vault/src/session.rs:383` (line 394 of the mutated file, which adds 11 lines above it; unit `session::tests::collection_cache_separates_reconversions`): `assertion failed: keys.collection_key(&different).is_none()`. `cargo test -p vault collection_cache_separates_reconversions` failed 0 passed/1 failed (44 filtered out) in 0.00s, build 17.80s.

State evidence: The unit caches a key under domain 0x41 through the public SessionKeys API and then looks up four identities that each differ in one field. Under the mutant's Eq/Hash, which drop domain_id, the owner, calendar and generation variants still miss, so the failing iteration is the domain 0x42 variant: the lookup returned the domain-0x41 key instead of None. Only the cache identity changed; authenticated_collection_keys' explicit (owner, calendar, domain) comparison and the grant-ring digest witness were untouched.

Ruling: Killed by the row's existing direct unit oracle (the live-preparation map names it as the intended detecting oracle for the public cache API): a key of an earlier conversion answers for a new key domain, exactly the condition. This attempt runs the exact unit only; attempt 2 runs the same patch under the whole za suite for case 24's third run and is recorded separately. Ring authentication was not weakened. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/vault/src/session.rs` | `e6cfb985ffc60ffbc9247506fe258d9754dad16cfac75d9035121bc3d2e09cb2` | `d695c3acc49cd1ce20c47e05434785306f4a74a86448a7c5ffba81b1fe822f74` | `e6cfb985ffc60ffbc9247506fe258d9754dad16cfac75d9035121bc3d2e09cb2` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/vault/src/session.rs
+++ b/crates/vault/src/session.rs
@@ -11,20 +11,31 @@
         Mutex, OnceLock, PoisonError,
         atomic::{AtomicU64, Ordering},
     },
 };

 /// Identity of one collection key. A new conversion uses a fresh domain,
 /// even when its calendar and generation numbers are reused.
-#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
+#[derive(Debug, Clone, Copy)]
 pub struct CollectionKeyId {
     pub owner_id: u32,
     pub calendar_id: u32,
     pub domain_id: [u8; 16],
     pub generation: u32,
+}
+impl PartialEq for CollectionKeyId {
+    fn eq(&self, other: &Self) -> bool {
+        (self.owner_id, self.calendar_id, self.generation) == (other.owner_id, other.calendar_id, other.generation)
+    }
+}
+impl Eq for CollectionKeyId {}
+impl std::hash::Hash for CollectionKeyId {
+    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
+        std::hash::Hash::hash(&(self.owner_id, self.calendar_id, self.generation), state);
+    }
 }

 /// Keys attached for one HTTP request (spec 5, "in-flight copies"; the HTTP
 /// key cache keeps them between requests under its idle and age limits,
 /// `ZA_KEY_IDLE_SECS` and `ZA_KEY_MAX_AGE_SECS`), or for the life of an
 /// authenticated IMAP or POP3 connection (mail spec 3.3, which amends
 /// release 1's "only HTTP requests carry keys"). WebSocket, EventSource,
```

## Mutation 36, attempt 2: killed

Required case: 24 third run (a session's collection-key cache keeps a key of an earlier conversion apart from the current one: the cache identity includes the key-domain id); attempt 2 runs attempt 1's patch under the whole za suite. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `63103d1404ca7013cdfcc8ea1712dfa4d414c2770e9a12aa36875b0c4b9e0eff`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 178.821s. Decisive observation:
`tests/src/za/sharing.rs:1701` (conversion_contract_tests, the class loop's cache block, first reached by the `revert` class on calendar `assert-revert`; `init` and `rewrap` continue at sharing.rs:1569 before it): `old cache identity must retain its key`, i.e. `keys.collection_key(&first_cache)` (the first conversion's domain, generation 1) returned bytes different from the key cached for that identity before the reconversion. The preceding sharing.rs:1697 (`new domain must publish a fresh CK`) passed. Whole ZA failed 0 passed/1 failed in 64.12s (build 1m 44s); last marker `Running conversion assertion, rollback, resume and cache contracts...`, after the `Mutation34 oracle: ...` line.

State evidence: Same patch bytes as attempt 1 (patch SHA256 equal). The fixture holds one resident owner SessionKeys across the first conversion (cut, then reverted and rolled back) and a reconversion that draws a new domain id (sharing.rs:1691 `assert_ne!(first_domain, new_domain)` passed). Owner resolution opens the ring freshly and publishes each generation's CK into the session cache; under the mutant's Eq/Hash without domain_id the new domain's generation-1 insert replaced the value held for the old domain's identity, so the old identity answered with the new key. The resident write and fresh-session read later in the block were not reached. Production reads were not the observation: owner resolution never reads CKs from this cache and grantee reuse keeps its exact grant-ring digest witness, so a stale-key read stays masked by fresh authentication (the Task 11 integration note); the fixture's public-API identity assertion is what observes the condition in the za suite.

Ruling: Killed at case 24's third run in the whole za suite, by its public SessionKeys/CollectionKeyId cache-identity assertion (the oracle Task 11's integration note added to that fixture to make this condition observable without weakening fresh authentication or the witness). The coordinator's note anticipated a possible whole-suite survival with the unit killing; the run shows the za fixture also detects it. Recorded separately from attempt 1 (exact unit), as instructed. Ring authentication was not weakened. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/vault/src/session.rs` | `e6cfb985ffc60ffbc9247506fe258d9754dad16cfac75d9035121bc3d2e09cb2` | `d695c3acc49cd1ce20c47e05434785306f4a74a86448a7c5ffba81b1fe822f74` | `e6cfb985ffc60ffbc9247506fe258d9754dad16cfac75d9035121bc3d2e09cb2` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/vault/src/session.rs
+++ b/crates/vault/src/session.rs
@@ -11,20 +11,31 @@
         Mutex, OnceLock, PoisonError,
         atomic::{AtomicU64, Ordering},
     },
 };

 /// Identity of one collection key. A new conversion uses a fresh domain,
 /// even when its calendar and generation numbers are reused.
-#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
+#[derive(Debug, Clone, Copy)]
 pub struct CollectionKeyId {
     pub owner_id: u32,
     pub calendar_id: u32,
     pub domain_id: [u8; 16],
     pub generation: u32,
+}
+impl PartialEq for CollectionKeyId {
+    fn eq(&self, other: &Self) -> bool {
+        (self.owner_id, self.calendar_id, self.generation) == (other.owner_id, other.calendar_id, other.generation)
+    }
+}
+impl Eq for CollectionKeyId {}
+impl std::hash::Hash for CollectionKeyId {
+    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
+        std::hash::Hash::hash(&(self.owner_id, self.calendar_id, self.generation), state);
+    }
 }

 /// Keys attached for one HTTP request (spec 5, "in-flight copies"; the HTTP
 /// key cache keeps them between requests under its idle and age limits,
 /// `ZA_KEY_IDLE_SECS` and `ZA_KEY_MAX_AGE_SECS`), or for the life of an
 /// authenticated IMAP or POP3 connection (mail spec 3.3, which amends
 /// release 1's "only HTTP requests carry keys"). WebSocket, EventSource,
```

## Mutation 7, attempt 3: killed

Required case: Case 21 (conversion contract, held-create variant), oracle R11-7b, row 7 narrowed to creates. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `4e54a33ec2cc9010d668895c120b813f5360809b547fbe63ef36ba38780d9529`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 108.817s. Decisive observation:
`tests/src/za/sharing.rs:2320`: assertion `left == right` failed: held-create: case 21 requires the held write's event to end under a 0x03 envelope; left: Some(1), right: Some(3).

State evidence: Create closure's calendar-assertion loop removed in crates/dav/src/calendar/update.rs (update loop kept); the held create committed under the stale EWK domain with a 0x01 envelope. Run reached 'conversion assertion, rollback, resume and cache contracts' and failed there about 68 s into the suite.

Ruling: Killed by the new oracle 07b (supplementary, creates-only narrowing, oracle-wave-1/README.md); first failure is the predicted assertion, line 2320, first (held-create) iteration. Applied via ROW7_CREATES_ONLY=1 in mutation_definitions.py.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `184ae26f557a7195a756dd69266aa9136dbf0a57dd5468ae09bbf5f1572dc4eb` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -791,17 +791,14 @@
                 |event: &CalendarEvent,
                  contexts: &[groupware::calendar::sharing::resolve::ZaCalendarContext]|
                  -> trc::Result<(BatchBuilder, Option<String>)> {
                     let mut batch = BatchBuilder::new();
                     if let Some((lease, _)) = &za_lease {
                         lease.lease().assert_in(&mut batch);
                     }
-                    for context in contexts {
-                        context.assert_in(&mut batch);
-                    }
                     let etag = event
                         .clone()
                         .insert_at(
                             access_token.account_tenant_ids(),
                             account_id,
                             document_id,
                             next_email_alarm.clone(),
```

## Mutation 11, attempt 2: killed

Required case: Case 6 (reader revoke rotation), oracle R11-11. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `90cb381d43c0872fd727132068292945279180641652ca05606c390d32a610c1`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 115.485s. Decisive observation:
`tests/src/za/sharing.rs:3380`: assertion `left == right` failed: case 6: a revoke must add exactly one generation; left: 1, right: 2.

State evidence: Same patch as attempt 1 (byte-identical mutation.patch). Run reached 'transactional sharing endpoint tests' and failed in sharing_rotation_contracts at the new oracle, about 62 s into the suite; the mutated revoke added no generation: the ring held 1 generation after the revoke where before+1 (2) was required.

Ruling: Killed by the new oracle (README: combined line 3380, individual 3335); first failure is the predicted assertion of oracle 11.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/mutations.rs` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` | `962be910efd6129ebc3dc77a466a394f261aadb0a870e1f9499ac19ebd5bb7e5` | `fb25f49ecca1e2ed4cfb029390fcff2ea49ee19c0f00a3d0d89d861031584a33` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/mutations.rs
+++ b/crates/groupware/src/calendar/sharing/mutations.rs
@@ -334,16 +334,19 @@
             applied.affected_grantees.push(account_id);
         }
     }
     if !applied.changed {
         return Ok(applied);
     }
     if applied.rotated {
-        header.generations.push(header.next_generation()?);
-        ring.keys.push(*Secret::random().as_bytes());
+        // Mutation11: no rotation on revoke; a revoke keeps the current generations.
+        if revoke.is_none() {
+            header.generations.push(header.next_generation()?);
+            ring.keys.push(*Secret::random().as_bytes());
+        }
         applied
             .affected_grantees
             .extend(ring.grants.iter().map(|g| g.account_id));
     }
     let opened = header
         .generations
         .iter()
```

## Mutation 15, attempt 2: killed

Required case: Case 11 (grantee gate controls, downgrade after the request snapshot), oracle R11-15. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `005797935ea68d095e9025a0971d99ee64ed6b86b499876a6cc7e59f63942c42`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 99.046s. Decisive observation:
`tests/src/za/sharing.rs:8593` (README: 8507 with wave 1 only, +86 from wave 2): assertion `left == right` failed: case 11: a grant downgraded after the request snapshot must be refused by the resolver's bit check; left: 204, right: 403.

State evidence: Same patch as attempt 1 (byte-identical mutation.patch). Held PUT parked at the put pause, the grant downgraded to view meanwhile; under the mutant the PUT committed (204) instead of being refused. Failed in 'Task 10 sync boundary, rename and private tombstone controls' phase (grantee_gate_controls), about 47 s into the suite.

Ruling: Killed by the new oracle; the failing assertion is the one README predicts (same message and values).

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/sharing/resolve.rs` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` | `e1997ef4e5a9c9a7664a92c4a11ea09d6e9dee9cc1dc287e6ed4e8476b9c9357` | `119a08055307400148a4b72a8c5ca2924b930e98e243a2cf1b813dc05625b86b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/sharing/resolve.rs
+++ b/crates/groupware/src/calendar/sharing/resolve.rs
@@ -155,15 +155,14 @@
     session(token, owner_id)?;
     if token.account_id() == owner_id {
         return Ok(());
     }
     if !za_foreign_calendar_eligible(token, &calendar.acls)
         || !calendar.acls.iter().any(|grant| {
             grant.account_id == token.account_id()
-                && grant.grants.bitmap & required.bitmap == required.bitmap
         })
         || !calendar
             .preferences
             .iter()
             .any(|entry| entry.account_id == token.account_id())
     {
         return Err(forbid(owner_id));
```

## Mutation 19, attempt 3: invalid/superseded qualification

Required case: Case 11 (corrupt eligible calendar fails the assisted own-home listing), oracle R11-19. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `0d57848f00ce3767b4b1770ca82d4b50326fcd6f0ded65dbcd410de0fbee60f7`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 102.074s. Decisive observation:
`tests/src/za/sharing.rs:8903` (README: 8817 with wave 1 only, +86 from wave 2): assertion `left == right` failed: case 11: a corrupt eligible calendar must fail the assisted own-home listing (<D:allprop/>): Ok(<multistatus body listing the home and shared calendar>); left: 207, right: 500.

State evidence: Same patch as attempt 2 (byte-identical mutation.patch). In grantee_unrelated_corruption, bob's own-home PROPFIND with <D:allprop/> answered 207 under the mutant where the unmutated code answers 500. Failed in 'Task 10 sync boundary, rename and private tombstone controls', about 44 s into the suite.

Historical ruling: the external coordinator called this killed under R11-19b. Review correction R11-19-review: invalid/superseded qualification. The207 response contains a500 item for the corrupt calendar, which the binding request/item500 contract permits. This records packaging only and does not qualify row19. Exact patch, assertion, command and restoration evidence remain below for history.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `00e9c1aeb913a71e80a1f40444d586baf68fcc6bb16fbaa2fed3946a1762cc5a` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1752,15 +1752,17 @@
                     if shared_account_id == access_token.account_id() {
                         continue;
                     }
                     let shared_resources = data
                         .resources(server, access_token, shared_account_id, sync_collection)
                         .await
                         .caused_by(trc::location!())?;
-                    let foreign_eligible = if sync_collection == SyncCollection::Calendar
+                    // Mutation19: the assisted listing skips the foreign eligibility check.
+                    let foreign_eligible = if false
+                        && sync_collection == SyncCollection::Calendar
                         && server
                             .za_is_key_account(shared_account_id)
                             .await
                             .caused_by(trc::location!())?
                     {
                         Some(
                             foreign_calendar_contexts(
```

## Mutation 29, attempt 2: killed

Required case: Case 1 (limit-1 sync, private tombstone not counted), oracle R11-29. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `5072e6473235f0871cdfdf0552f8209c2500ff2e9380be92b31bd6e75d52d04c`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 86.343s. Decisive observation:
`tests/src/za/sharing.rs:7144`: limit-1 sync of /dav/cal/sharing-dav-alice%40example.com/ answered continuation urn:stalwart:davsync:1d:1: private tombstone counted (message-only assert!, home target, first iteration).

State evidence: Same patch as attempt 1 (byte-identical mutation.patch). The earlier existing assertions (granted tombstone present, no /private/ href) passed; the private tombstone counted toward the limit and truncated the response into a continuation token. Failed in 'Task 10 sync boundary, rename and private tombstone controls', about 45 s into the suite.

Ruling: Killed by the new oracle; the failing assertion and message are exactly those README predicts (patched 7144, home target).

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `0e7162bad066825fdec36113d7a691c462236ba4696371f6cb1535c9f961c91c` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -1461,15 +1461,24 @@
                     "{}{}/",
                     resources.base_path,
                     percent_encoding::utf8_percent_encode(&calendar.name, groupware::RFC_3986)
                 ));
             }
         }
         if let Some(vanished) = &mut foreign_vanished {
-            vanished.retain(|path| prefixes.iter().any(|prefix| path.starts_with(prefix)));
+            // Mutation29 leaves vanished paths unfiltered.
+            let mutation29_dropped = vanished
+                .iter()
+                .filter(|path| !prefixes.iter().any(|prefix| path.starts_with(prefix.as_str())))
+                .count();
+            eprintln!(
+                "Mutation29 vanished paths unfiltered: total={}, outside granted prefixes={}",
+                vanished.len(),
+                mutation29_dropped
+            );
         }
         // Reuse precisely these freshly read archives for the rest of this response.
         for (&id, context) in eligible {
             data.calendar_memo.insert((account_id, id), context.clone());
         }
     }

```

## Mutation 31, attempt 2: killed

Required case: Case 1 (rename into a former slug, unpaused; kept-token sync after the rename boundary), oracle R11-31. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `d142dead866cb1c3a45b2e44b6326182d980b0b958d69cede134336ed2928201`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 90.292s. Decisive observation:
`tests/src/utils/webdav.rs:599` (DavResponse::with_status, no #[track_caller]; called from the kept-token sync at sharing.rs about 7214): Expected 403 Forbidden but got 207 Multi-Status.

State evidence: Same patch as attempt 1 (byte-identical mutation.patch). Marker 'Task11 rename into a former slug, unpaused' printed (run.log:47) before the failure, so this is the new unpaused block and not the paused-rename block; the shared calendar's boundary was not advanced, so bob's sync from the kept token was admitted (207) instead of 403. Failed about 43 s into the suite, before the 5 s SHARE_BOUNDARY_PAUSE wait attempt 1 hit.

Ruling: Killed by the new oracle; the failing assertion and message are those README predicts (home iteration).

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/copy_move.rs` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` | `6675185de4e78dfbd00b149ca3cfee999ef65fa5993651ff67f63f3a0c26faad` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/copy_move.rs
+++ b/crates/dav/src/calendar/copy_move.rs
@@ -1183,14 +1183,17 @@
         .deserialize::<Calendar>()
         .caused_by(trc::location!())?;
     new_calendar.name = new_name.to_string();
     let shared = context
         .as_ref()
         .is_some_and(|context| context.domain.calendar_id().is_some());
     if shared {
+        eprintln!("Mutation31 shared calendar rename leaves its boundary unchanged: calendar={document_id}");
+    }
+    if false && shared {
         groupware::calendar::sharing::boundary::mark_boundary_pending(
             &mut new_calendar,
             &context.as_ref().unwrap().domain,
         )
         .map_err(|error| groupware::calendar::seal::seal_error(error, account_id, document_id))?;
     }

@@ -1207,15 +1210,15 @@
     batch.log_vanished_item(VanishedCollection::Calendar, from_resource_path);
     #[cfg(feature = "test_mode")]
     common::za::test_hooks::share_rename_pause_point(account_id).await;
     let assigned = server
         .commit_batch(batch)
         .await
         .caused_by(trc::location!())?;
-    if shared {
+    if false && shared {
         groupware::calendar::sharing::boundary::za_complete_boundary(
             server,
             access_token
                 .session_keys()
                 .ok_or(DavError::Code(StatusCode::FORBIDDEN))?,
             document_id,
             Some(groupware::calendar::sharing::boundary::ZaBoundaryCommit {
```

## Mutation 37, attempt 1: killed

Required case: 10 (self-delivery is not contingent on the first run: sole-attendee decline, fixture case10-sole-decline). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `df80fa5237881489a8077a76e31de53e598e86e631ce03da50b6b16982b9483a`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 101.455s. Decisive observation:
`sharing.rs:9523` (sharing_http_schedule_contracts, case 10, sole-attendee loop, fixture `case10-sole-decline`): `assertion left == right failed`, left `Some("NEEDS-ACTION")`, right `Some("DECLINED")`: after Bob set his own PARTSTAT to DECLINED through Alice's shared event and the drain step ran, his own copy still read NEEDS-ACTION. Whole ZA failed 0 passed/1 failed in 51.67s (build 38.54s); last marker `Running case25 HTTP owner/delegated lifecycle differential...` (case 10 runs inside it, after case 25's steps).

State evidence: Scoped by exact event UID per ruling R11-37: the second run's output is dropped unless the first run built, at both PUT branches (update and create), only when the tree's first UID is `case10-sole-decline`; every other tree keeps the original. The trace printed 34 unscoped decisions (case 25's labels, case 9, case10-participation and case10-alias-replies; among them sharing-force-sole-delegated create and case10-alias-replies create with first_built=false, second=true, which the original kept) and exactly one scoped decision: `update uid=case10-sole-decline scoped=true first_built=false second=true` (Bob's DECLINED write: the first run built nothing, the second run built Bob's copy, which the mutant dropped). No fan-out followed, so Bob's own copy stayed NEEDS-ACTION. Restored update.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 10: the run failed at the sole-decline assertion with exactly the state the condition explains (the self builder's output gated on the first run's, so the sole-attendee DECLINED write delivers no own copy). The predicted earlier case 25 detection (force-sole) was not executed and is not claimed (R11-37). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `06bd1405a945c786b8eb13e3eff72ace09e2c264b3c23cdef8e44d5fd1a3ed05` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -336,14 +336,30 @@
                         &old_ical,
                         account_info.addresses(),
                     )
                 } else {
                     itip_create(&mut new_event.data.event, account_info.addresses())
                 };
                 let first_built = result.is_ok();
+                // Mutation37: self-delivery only when the first run built messages.
+                let mutation37_scoped = new_event.data.event.uids().next() == Some("case10-sole-decline");
+                if za_resolution.is_some() {
+                    eprintln!(
+                        "Mutation37 second-run gate (update): uid={:?}, scoped={}, first_built={}, second={}",
+                        new_event.data.event.uids().next(),
+                        mutation37_scoped,
+                        first_built,
+                        second.is_some()
+                    );
+                }
+                let second = if first_built || !mutation37_scoped {
+                    second
+                } else {
+                    None
+                };
                 let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
                 let first = match result {
                     Ok(messages) => Some(messages),
                     Err(err) => {
                         if let Some(failed_precondition) = err.failed_precondition() {
                             return Err(DavError::Condition(
                                 DavErrorCondition::new(
@@ -646,14 +662,30 @@
                 let second = za_resolution.as_ref().and_then(|resolution| {
                     za_self_create(&event.data.event, writer_account, event_account, resolution)
                         .ok()
                         .flatten()
                 });
                 let result = itip_create(&mut event.data.event, account_info.addresses());
                 let first_built = result.is_ok();
+                // Mutation37: self-delivery only when the first run built messages.
+                let mutation37_scoped = event.data.event.uids().next() == Some("case10-sole-decline");
+                if za_resolution.is_some() {
+                    eprintln!(
+                        "Mutation37 second-run gate (create): uid={:?}, scoped={}, first_built={}, second={}",
+                        event.data.event.uids().next(),
+                        mutation37_scoped,
+                        first_built,
+                        second.is_some()
+                    );
+                }
+                let second = if first_built || !mutation37_scoped {
+                    second
+                } else {
+                    None
+                };
                 let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
                 let first = match result {
                     Ok(messages) => messages,
                     Err(err) => {
                         if let Some(failed_precondition) = err.failed_precondition() {
                             return Err(DavError::Condition(
                                 DavErrorCondition::new(
```

## Mutation 38, attempt 1: killed

Required case: 10 (no self-CANCEL on a delegated deletion: Bob deletes the sole-attendee event, fixture case10-sole-decline). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `2be980fdac0ff8defff1921612105b26fa1b4434eb5841c246f702f0ec5f01f8`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 116.275s. Decisive observation:
`sharing.rs:9546` (sharing_http_schedule_contracts, case 10, sole-attendee loop, first iteration `case10-sole-decline`): `assertion failed: body(&bob.client, &own).await.contains("STATUS:CANCELLED")`: after Bob's DELETE of Alice's shared event (204) and the drain step, Bob's own copy carried no STATUS:CANCELLED. Whole ZA failed 0 passed/1 failed in 48.74s (build 54.61s); last marker `Running case25 HTTP owner/delegated lifecycle differential...` (case 10 runs inside it, after case 25's steps).

State evidence: Scoped by UID prefix per ruling R11-38: the self-CANCEL builder (`za_self_cancel`) is skipped, so the second run contributes nothing, only for delegated deletions whose `message.uid` starts with `case10-`; every other delegated deletion keeps the original. The trace printed seven unscoped delegated deletions (sharing-eleven-delegated, sharing-listed-alias-delegated, sharing-subaddress-delegated, sharing-subaddress-only-delegated, sharing-force-sole-delegated, case9-delegated, case9-cancel-gain; case 25 and case 9 passed) and exactly one scoped one, `uid=case10-sole-decline scoped=true`, immediately before the failure. With Bob the sole attendee, the first run (merged list) has no non-writer recipient, so without the self-CANCEL no fan-out reaches Bob's own copy. Restored delete.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 10: the run failed at the deletion assertion with exactly the state the condition explains (no self-CANCEL, so the writer's own copy is not cancelled after the source is gone). The predicted earlier case 25 detection was not executed and is not claimed (R11-38). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/delete.rs` | `1e94fadafe67490990e2714725ac4f2ac3da5b089a793b3901df8568a6cad66b` | `9b446d00832ce9342cbefa0e9643539e0763047ef2ebd65a52cd0f1589483461` | `1e94fadafe67490990e2714725ac4f2ac3da5b089a793b3901df8568a6cad66b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/delete.rs
+++ b/crates/dav/src/calendar/delete.rs
@@ -334,18 +334,27 @@
                     event_account,
                     ZaSelfOperation::Delete,
                     Some(&message.event),
                     None,
                 )
                 .await?;
                 let messages = if let Some(resolution) = &resolution {
-                    let second =
+                    // Mutation38: no self-CANCEL.
+                    let mutation38_scoped = message.uid.starts_with("case10-");
+                    eprintln!(
+                        "Mutation38 delegated delete: uid={}, scoped={}",
+                        message.uid, mutation38_scoped
+                    );
+                    let second = if mutation38_scoped {
+                        None
+                    } else {
                         za_self_cancel(&message.event, writer_account, event_account, resolution)
                             .ok()
-                            .flatten();
+                            .flatten()
+                    };
                     za_combine_delegated(message.message.into_iter().collect(), second, resolution)
                 } else {
                     message
                         .message
                         .into_iter()
                         .map(ZaFanOutMessage::upstream)
                         .collect()
```

## Mutation 39, attempt 1: killed

Required case: 25 (second run judged by the merged list instead of owner locality: every step). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `9340f05eb14937255285b28c3ebe27c4e7efe00a9769e6ac8c69e1c5c9b6e0ee`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 111.65s. Decisive observation:
`sharing.rs:7726` (sharing_compare_lifecycle via sharing_lifecycle_differential, case 25 label `eleven`, step 0, the create): `assertion left == right failed: eleven step0: inbox payload/address multisets`, left (owner run) one REQUEST item in Bob's inbox addressed to `["sharing-life-bob@example.com", "sharing-life-carol@example.com"]`, right (delegated run) `[]`. Whole ZA failed 0 passed/1 failed in 45.26s (build 55.28s); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: No scope: the owner snapshot of the second run (`owner_snapshot`) passes owner addresses plus L (`resolution.listed`) to `itip_snapshot` instead of the owner's addresses, so the listed writer address Bob is local in the second run; the second run then builds nothing for Bob, and the first run already had Bob removed (he is in R), so Bob's inbox gets no new item in the delegated run while the owner run delivers his REQUEST. The first delegated write of the suite is case 25's `eleven` create; no earlier module reaches the delegated path. Restored delegation.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 25, step 0 of `eleven`, the row's required case: the owner/delegated differential sees the writer's missing own message, exactly the state the condition explains. A unit supplement (attempt 2) runs the same patch against second_run_uses_owner_locality. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `bbe0daae508377c0c328cb4ba59cc04de9073dbdc5f5192e2bc4fdf73b2953aa` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -227,15 +227,18 @@
 static FORCE_REQUEST: ICalendarScheduleForceSendValue = ICalendarScheduleForceSendValue::Request;

 fn owner_snapshot<'a>(
     tree: &'a ICalendar,
     owner: &AccountInfo,
     resolution: &ZaDelegationResolution,
 ) -> Result<ItipSnapshots<'a>, ItipError> {
-    let mut snapshot = itip_snapshot(tree, &owner.addresses, false)?;
+    // Mutation39: second-run locality by the merged list (owner addresses plus L).
+    let mut mutation_merged = owner.addresses.clone();
+    mutation_merged.extend(resolution.listed.iter().cloned());
+    let mut snapshot = itip_snapshot(tree, &mutation_merged, false)?;
     for instance in snapshot.components.values_mut() {
         // `Attendee` is a hash-set key. Remove and rebuild rather than changing
         // one of its fields while it is inside the set.
         let attendees = std::mem::take(&mut instance.attendees);
         instance.attendees = attendees
             .into_iter()
             .map(|mut attendee| {
```

## Mutation 39, attempt 2: killed

Required case: 25 (unit supplement: groupware calendar::delegation::tests::second_run_uses_owner_locality, same patch as attempt 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `9340f05eb14937255285b28c3ebe27c4e7efe00a9769e6ac8c69e1c5c9b6e0ee`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test -p groupware calendar::delegation::tests::second_run_uses_owner_locality -- --nocapture
```

Exit `101`, elapsed wall time 33.001s. Decisive observation:
`crates/groupware/src/calendar/delegation.rs:494` in the mutated file (the `to` assertion of second_run_uses_owner_locality, 491 at HEAD): `assertion left == right failed`, left `["catch@example.com", "custom@example.com", "writer+tag@example.com"]`, right `["catch@example.com", "custom@example.com", "writer+tag@example.com", "writer@example.com", "writers@example.com"]`. Unit result 0 passed/1 failed (145 filtered out); build 26.73s.

State evidence: Same patch as attempt 1 (identical mutation.patch). Under the merged list, the listed writer addresses (WRITER and the GROUP address) become local in the owner snapshot, so the second run omits them from its recipients.

Ruling: Supplementary unit kill on the exact builder rule; attempt 1 holds the required case 25 HTTP evidence, which this unit does not replace. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `bbe0daae508377c0c328cb4ba59cc04de9073dbdc5f5192e2bc4fdf73b2953aa` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -227,15 +227,18 @@
 static FORCE_REQUEST: ICalendarScheduleForceSendValue = ICalendarScheduleForceSendValue::Request;

 fn owner_snapshot<'a>(
     tree: &'a ICalendar,
     owner: &AccountInfo,
     resolution: &ZaDelegationResolution,
 ) -> Result<ItipSnapshots<'a>, ItipError> {
-    let mut snapshot = itip_snapshot(tree, &owner.addresses, false)?;
+    // Mutation39: second-run locality by the merged list (owner addresses plus L).
+    let mut mutation_merged = owner.addresses.clone();
+    mutation_merged.extend(resolution.listed.iter().cloned());
+    let mut snapshot = itip_snapshot(tree, &mutation_merged, false)?;
     for instance in snapshot.components.values_mut() {
         // `Attendee` is a hash-set key. Remove and rebuild rather than changing
         // one of its fields while it is inside the set.
         let attendees = std::mem::take(&mut instance.attendees);
         instance.attendees = attendees
             .into_iter()
             .map(|mut attendee| {
```

## Mutation 40, attempt 1: killed

Required case: 25 (second run placed after the first run's finalize: the DTSTART edit). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `8259a889c0452878e4e1d23c4a563fc7278ce33ee4a22c8a9e547602d9481a82`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 107.528s. Decisive observation:
`sharing.rs:7726` (sharing_compare_lifecycle via sharing_lifecycle_differential, case 25 label `eleven`, step 2, the DTSTART edit): `assertion left == right failed: eleven step2: inbox payload/address multisets`; Bob's inbox item addressed to `["sharing-life-bob@example.com", "sharing-life-carol@example.com"]` in both runs, but the owner run's REQUEST carries `SEQUENCE:1` on both VEVENTs and the delegated run's `SEQUENCE:2` (first difference at the master's Sequence entry). Whole ZA failed 0 passed/1 failed in 47.53s (build 50.38s); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: No scope; the audit's corrected definition moves the second run after the first run in both PUT branches (update: after `itip_update`/`itip_create`; create: after `itip_create`). Steps 0 and 1 (create, SUMMARY change) passed, as predicted: the create has no SCHEDULE-FORCE-SEND and SUMMARY is not an increment property. At step 2 the first run's `itip_update` already incremented SEQUENCE on the real tree; the second run, now reading the finalized tree against the old one, increments again, so Bob's delegated copy is one SEQUENCE ahead of the owner run's. Restored update.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 25, the DTSTART edit, the row's required case: the n+1 versus n+2 SEQUENCE difference is exactly the state the condition explains. No unit supplement: second_run_precedes_finalize calls za_self_update directly and is insensitive to this orchestration move (audit). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `650aae3e2257250a5cad0d29d69bfe83f1aa924b50a6d84ebd45ccdd6e84d8ab` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -305,15 +305,24 @@
                             ZaSelfOperation::Create
                         },
                         Some(&old_ical),
                         Some(&new_event.data.event),
                     )
                     .await?;
                 }
-                // The second run sees the pre-finalize trees.
+                let result = if is_update {
+                    itip_update(
+                        &mut new_event.data.event,
+                        &old_ical,
+                        account_info.addresses(),
+                    )
+                } else {
+                    itip_create(&mut new_event.data.event, account_info.addresses())
+                };
+                // Mutation40: the second run sees the finalized tree.
                 let second = za_resolution.as_ref().and_then(|resolution| {
                     if is_update {
                         za_self_update(
                             &old_ical,
                             &new_event.data.event,
                             writer_account,
                             event_account,
@@ -326,23 +335,14 @@
                             event_account,
                             resolution,
                         )
                     }
                     .ok()
                     .flatten()
                 });
-                let result = if is_update {
-                    itip_update(
-                        &mut new_event.data.event,
-                        &old_ical,
-                        account_info.addresses(),
-                    )
-                } else {
-                    itip_create(&mut new_event.data.event, account_info.addresses())
-                };
                 let first_built = result.is_ok();
                 let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
                 let first = match result {
                     Ok(messages) => Some(messages),
                     Err(err) => {
                         if let Some(failed_precondition) = err.failed_precondition() {
                             return Err(DavError::Condition(
@@ -639,20 +639,21 @@
                         event_account,
                         ZaSelfOperation::Create,
                         None,
                         Some(&event.data.event),
                     )
                     .await?;
                 }
+                let result = itip_create(&mut event.data.event, account_info.addresses());
+                // Mutation40: the second run sees the finalized tree.
                 let second = za_resolution.as_ref().and_then(|resolution| {
                     za_self_create(&event.data.event, writer_account, event_account, resolution)
                         .ok()
                         .flatten()
                 });
-                let result = itip_create(&mut event.data.event, account_info.addresses());
                 let first_built = result.is_ok();
                 let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
                 let first = match result {
                     Ok(messages) => messages,
                     Err(err) => {
                         if let Some(failed_precondition) = err.failed_precondition() {
                             return Err(DavError::Condition(
```

## Mutation 41, attempt 1: killed

Required case: 25 (upstream's public builder run on the real tree as the second run: the DTSTART edit, double SEQUENCE increment). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `299969b4527f896cace7421df5d2e15753b5683e25721230cc9aaba7bc063757`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 95.962s. Decisive observation:
`sharing.rs:7726` (sharing_compare_lifecycle via sharing_lifecycle_differential, case 25 label `eleven`, step 2, the DTSTART edit): `assertion left == right failed: eleven step2: inbox payload/address multisets`; Bob's inbox item addressed to `["sharing-life-bob@example.com", "sharing-life-carol@example.com"]` in both runs, but the owner run's REQUEST carries `SEQUENCE:1` on both VEVENTs and the delegated run's `SEQUENCE:2` (first difference at the master's Sequence entry). Whole ZA failed 0 passed/1 failed in 45.75s (build 42.88s, one expected warning: unused import `za_self_update`); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: No scope; the audit's corrected definition. A temporary `za_self_update_public(old, new: &mut ICalendar, ...)` in delegation.rs keeps the production gate (`delegated_organizer`) and writer filter (`retain_writer_messages`) and calls upstream's public `itip_update(new, old, &owner.addresses)`; the update branch of update.rs calls it with `&mut new_event.data.event`, the real tree (confirmed in the reviewed patch: no clone), so the public builder finalizes and increments that tree before the first run's `itip_update` increments it again. Steps 0 and 1 (create, SUMMARY change) passed, as predicted. Restored delegation.rs and update.rs SHA256 equal pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 25, the DTSTART edit, the row's required case: the double SEQUENCE increment (delegated copy at 2, owner run at 1) is exactly the state the condition explains. No unit supplement: second_run_precedes_finalize calls the unchanged za_self_update and is insensitive (audit). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `08a2b0753541acdfe24f11b7487e3786674d2d1291543f123e201a5979554344` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `87cdcafa9c98f65b128e44815b841be49d93916a0b87694f63f4aed8bae1633b` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -292,14 +292,29 @@
         Err(ItipError::NotOrganizer)
     } else {
         organizer_request_full(new, &snapshot, None, true)
             .map(|messages| Some(retain_writer_messages(messages, vec![], true, resolution)))
     }
 }

+/// Mutation41: upstream's public builder on the real (mutable) tree.
+pub fn za_self_update_public(
+    old: &ICalendar,
+    new: &mut ICalendar,
+    writer: &AccountInfo,
+    owner: &AccountInfo,
+    resolution: &ZaDelegationResolution,
+) -> Result<Option<ZaSelfMessages>, ItipError> {
+    if !delegated_organizer(old, writer, owner, resolution) {
+        return Ok(None);
+    }
+    crate::scheduling::event_update::itip_update(new, old, &owner.addresses)
+        .map(|messages| Some(retain_writer_messages(messages, vec![], false, resolution)))
+}
+
 pub fn za_self_update(
     old: &ICalendar,
     new: &ICalendar,
     writer: &AccountInfo,
     owner: &AccountInfo,
     resolution: &ZaDelegationResolution,
 ) -> Result<Option<ZaSelfMessages>, ItipError> {
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -308,17 +308,17 @@
                         Some(&new_event.data.event),
                     )
                     .await?;
                 }
                 // The second run sees the pre-finalize trees.
                 let second = za_resolution.as_ref().and_then(|resolution| {
                     if is_update {
-                        za_self_update(
+                        groupware::calendar::delegation::za_self_update_public(
                             &old_ical,
-                            &new_event.data.event,
+                            &mut new_event.data.event,
                             writer_account,
                             event_account,
                             resolution,
                         )
                     } else {
                         za_self_create(
                             &new_event.data.event,
```

## Mutation 42, attempt 1: killed

Required case: 25 (force_send set alone, RSVP kept: the exception's run, direct listed alias with RSVP=FALSE). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `297b141dc8234f28f1bd97e3a2aaa9ee1117acfb4bf1660f0a8024e9632975a3`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 98.964s. Decisive observation:
`sharing.rs:7834` (sharing_compare_lifecycle via sharing_lifecycle_negative_and_exception, label `exception-NEEDS-ACTION`, step 0, flags `PARTSTAT=NEEDS-ACTION;RSVP=FALSE`): panic `delegated exception delivers the direct listed alias`: the delegated run produced no own copy for the direct listed alias. Whole ZA failed 0 passed/1 failed in 51.85s (build 39.79s); last marker `Running case25 HTTP owner/delegated lifecycle differential...`. The failing label is identified by order: the per-step trace (printed at sharing.rs:7865, after this assertion) last shows `negative-catchall-DECLINED step1`, the final label of the DECLINED flag set, and the next label is `exception-NEEDS-ACTION`.

State evidence: No scope: in `owner_snapshot`, an override alias (A) gets `force_send = REQUEST` but keeps its RSVP. Upstream's `send_invite_messages` skips an attendee whose `rsvp == Some(false)`, so the RSVP=FALSE alias gets nothing from the second run. Every earlier A alias carries RSVP=TRUE, so nothing earlier changed: the traces show `exception-DECLINED` steps 0-2 with delegated copies `[false, true]` and messages `[0, 1]` as in the original, case 25's main labels, case 9 and case 10 passed. Restored delegation.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 25, the exception's run, the row's required case: the missing direct-alias copy under RSVP=FALSE is exactly the state the condition explains. A unit supplement (attempt 2) runs the same patch against snapshot_exception_leaves_scheduling_agent_and_payload_untouched. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `610645e77c0964a12466ee0d0e06b667591d3c3938f7f734199ed1a3594e226e` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -240,15 +240,15 @@
             .into_iter()
             .map(|mut attendee| {
                 if !attendee.email.is_local
                     && za_normalize_address(&attendee.email.email)
                         .is_some_and(|key| resolution.override_aliases.contains(&key))
                 {
                     attendee.force_send = Some(&FORCE_REQUEST);
-                    attendee.rsvp = None;
+                    // Mutation42 keeps RSVP while force_send is overridden.
                 }
                 attendee
             })
             .collect();
     }
     Ok(snapshot)
 }
```

## Mutation 42, attempt 2: killed

Required case: 25 (unit supplement: groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched, same patch as attempt 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `297b141dc8234f28f1bd97e3a2aaa9ee1117acfb4bf1660f0a8024e9632975a3`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test -p groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched -- --nocapture
```

Exit `101`, elapsed wall time 7.742s. Decisive observation:
`crates/groupware/src/calendar/delegation.rs:619` (same line at HEAD; the patch replaces one line by one): `assertion left == right failed`, left `0`, right `1`: `assert_eq!(actual.messages().len(), 1)` after `za_self_create(...).unwrap().unwrap()` for the RSVP=FALSE exception tree. Unit result 0 passed/1 failed (145 filtered out); build 4.09s.

State evidence: Same patch as attempt 1 (identical mutation.patch). The builder returned Ok with no retained message for the direct listed alias (the audit had predicted an unwrap failure on NothingToSend; the actual outcome is an empty retained set, the same condition seen through the message count).

Ruling: Supplementary unit kill on the exact snapshot rule; attempt 1 holds the required case 25 HTTP evidence, which this unit does not replace. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `610645e77c0964a12466ee0d0e06b667591d3c3938f7f734199ed1a3594e226e` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -240,15 +240,15 @@
             .into_iter()
             .map(|mut attendee| {
                 if !attendee.email.is_local
                     && za_normalize_address(&attendee.email.email)
                         .is_some_and(|key| resolution.override_aliases.contains(&key))
                 {
                     attendee.force_send = Some(&FORCE_REQUEST);
-                    attendee.rsvp = None;
+                    // Mutation42 keeps RSVP while force_send is overridden.
                 }
                 attendee
             })
             .collect();
     }
     Ok(snapshot)
 }
```

## Mutation 43, attempt 1: killed

Required case: 10 (schedule tag left unset when only the second run built: Bob creates the sole-attendee event, fixture case10-sole-delegated). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `704ff26ce81976ad1f32c33a636703883b73859722cf22330e570392829e34ed`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 96.507s. Decisive observation:
`sharing.rs:9546` (sharing_http_schedule_contracts, case 10, sole-attendee loop, second iteration `case10-sole-delegated`): `assertion failed: body(&bob.client, &own).await.contains("STATUS:CANCELLED")`: after Bob's DELETE (204) of the event he created and updated, and the drain step, Bob's own copy carried no STATUS:CANCELLED. The create (201), the own-copy lookup and the SUMMARY update assertion (`SUMMARY:sole updated` in Bob's copy) passed before it. Whole ZA failed 0 passed/1 failed in 48.20s (build 40.95s); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: Scoped by exact event UID per ruling R11-43: the two self-only schedule-tag assignments (the PUT create branch's `event.schedule_tag = Some(1)` when the first run did not build, and the existing-resource branch's `else if new_event.schedule_tag.is_none()` assignment) are skipped only when the tree's first UID is `case10-sole-delegated`. The trace printed two unscoped second-only decisions (create `sharing-force-sole-delegated`, create `case10-alias-replies`; both kept the original tag) and two scoped ones: `create uid=case10-sole-delegated scoped=true` (Bob's create: tag left None) and `update uid=case10-sole-delegated scoped=true` (the SUMMARY PUT, which with no tag took the create dispatch, still delivered `sole updated`, and again left the tag unset). The deletion then builds its CANCEL only when `schedule_tag.is_some()` (calendar/storage.rs), so no CANCEL reached Bob's copy; the first iteration (`case10-sole-decline`) passed unchanged. Restored update.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 10: the run failed at the later deletion with exactly the state the condition explains (no schedule tag after a self-only create, so the deletion builds no CANCEL). The predicted earlier case 25 detection (force-sole tag assertion) was not executed and is not claimed (R11-43). Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `2334266160d3f2a24107d931e13823b553b64ea48be5b5eeb67e6c178db35c9e` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -381,15 +381,24 @@
                         if first_built {
                             if let Some(schedule_tag) = &mut new_event.schedule_tag {
                                 *schedule_tag += 1;
                             } else {
                                 new_event.schedule_tag = Some(1);
                             }
                         } else if new_event.schedule_tag.is_none() {
-                            new_event.schedule_tag = Some(1);
+                            // Mutation43: tag left unset when only the second run built.
+                            let mutation43_scoped = new_event.data.event.uids().next() == Some("case10-sole-delegated");
+                            eprintln!(
+                                "Mutation43 second-only tag (update): uid={:?}, scoped={}",
+                                new_event.data.event.uids().next(),
+                                mutation43_scoped
+                            );
+                            if !mutation43_scoped {
+                                new_event.schedule_tag = Some(1);
+                            }
                         }
                     }
                     let first = first.unwrap_or_default();
                     if itip_status.is_send_sealed() {
                         let messages = if let Some(resolution) = &za_resolution {
                             za_combine_delegated(first, second, resolution)
                         } else {
@@ -674,15 +683,26 @@
                 if first_built || second.is_some() {
                     if first_nothing
                         && let Some(second) = &second
                         && second.finalize()
                     {
                         za_finalize(&mut event.data.event, second.increments());
                     }
-                    event.schedule_tag = Some(1);
+                    // Mutation43: tag left unset when only the second run built.
+                    let mutation43_scoped = event.data.event.uids().next() == Some("case10-sole-delegated");
+                    if !first_built {
+                        eprintln!(
+                            "Mutation43 second-only tag (create): uid={:?}, scoped={}",
+                            event.data.event.uids().next(),
+                            mutation43_scoped
+                        );
+                    }
+                    if first_built || !mutation43_scoped {
+                        event.schedule_tag = Some(1);
+                    }
                     if itip_status.is_send_sealed() {
                         let messages = if let Some(resolution) = &za_resolution {
                             za_combine_delegated(first, second, resolution)
                         } else {
                             first.into_iter().map(ZaFanOutMessage::upstream).collect()
                         };
                         if messages
```

## Mutation 44, attempt 1: killed

Required case: 25 (writer delivered from the first run's output instead of the second run's). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `ae23c1b428a157078454f81be6c4b96e4aacdbdf8a64025b2a90d7110b76de11`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 103.98s. Decisive observation:
`sharing.rs:7726` (sharing_compare_lifecycle via sharing_lifecycle_differential, case 25 label `eleven`, step 0, the create): `assertion left == right failed: eleven step0: inbox payload/address multisets`, left (owner run) one REQUEST item in Bob's inbox addressed to `["sharing-life-bob@example.com", "sharing-life-carol@example.com"]`, right (delegated run) `[]`. Whole ZA failed 0 passed/1 failed in 45.64s (build 48.35s; only the inherited linker eh_frame warning); last marker `Running case25 HTTP owner/delegated lifecycle differential...`.

State evidence: No scope: `za_combine_delegated` returns the first run's messages unfiltered by R, each tagged DelegatedSelf, and drops the second run. The first run is built under the merged list, in which Bob's listed address is local, so it holds no message for Bob; without the owner-locality second run, Bob's inbox gets nothing in the delegated run while the owner run delivers his REQUEST. The first delegated write of the suite is case 25's `eleven` create. Restored delegation.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at case 25, step 0 of `eleven`, the row's required case: the owner/delegated differential sees the writer's missing own message, exactly the state the condition explains. No unit listed. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `4c449ca8627d08756ad58dfd9e69770e7ded7726340c525afe73bcc6950fdd4f` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -169,33 +169,20 @@
 /// Remove writer recipients from the first run exactly once, then append the
 /// owner-locality run. Only this function can create delegated-self provenance.
 pub fn za_combine_delegated(
     first: Vec<ItipMessage<ICalendar>>,
     second: Option<ZaSelfMessages>,
     resolution: &ZaDelegationResolution,
 ) -> Vec<ZaFanOutMessage> {
-    let mut combined = first
+    // Mutation44: the writer gets the first run's output, not the second run's.
+    let _ = (second, resolution);
+    first
         .into_iter()
-        .filter_map(|mut message| {
-            message.to.retain(|address| {
-                !za_normalize_address(address)
-                    .is_some_and(|key| resolution.recipients.contains(&key))
-            });
-            (!message.to.is_empty()).then(|| ZaFanOutMessage::upstream(message))
-        })
-        .collect::<Vec<_>>();
-    if let Some(second) = second {
-        combined.extend(
-            second
-                .messages
-                .into_iter()
-                .map(ZaFanOutMessage::delegated_self),
-        );
-    }
-    combined
+        .map(ZaFanOutMessage::delegated_self)
+        .collect::<Vec<_>>()
 }

 pub fn za_finalize(tree: &mut ICalendar, increments: &[u16]) {
     crate::scheduling::itip::itip_finalize(tree, increments);
 }

 /// The organizer gate is evaluated on the new tree for create, and on the
```

## Mutation 45, attempt 1: killed

Required case: 25 (SEQUENCE normalized away by the differential helper; detected by baseline control #45 before case 25's steps). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `662e451d0f6b9ecb3a56ce85f46286edd81f4f5c25300d948b8a469750dacec0`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 92.72s. Decisive observation:
`sharing.rs:7907` (sharing_lifecycle_differential, first assertion, baseline control #45): panic `mutation45: normalization must preserve an n+2 SEQUENCE difference`: `normalize_sharing_copy` made the SEQUENCE:7 and SEQUENCE:9 trees equal. Whole ZA failed 0 passed/1 failed in 46.19s (build 39.29s); last marker `Running case25 HTTP owner/delegated lifecycle differential...`, printed immediately before the control.

State evidence: No scope: the mutation adds `ICalendarProperty::Sequence` to the properties `normalize_sharing_copy` (tests/src/utils/za.rs, a test helper; the driver saved and restored it like any source) strips, so case 25's owner/delegated equality could no longer see an n+1 versus n+2 SEQUENCE difference. The control runs at the top of case 25 before any delegated write, so nothing earlier masks it. Restored tests/src/utils/za.rs SHA256 equals pre; the working-tree status matches pre-status.txt.

Ruling: Killed at the case 25 differential's own control: the helper rule the row names is the oracle case 25 relies on, and control #45 fails with exactly the state the condition explains. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `tests/src/utils/za.rs` | `ad2ba423b697a7d22000c53f4d7961d28397105d0d2e6cc6ab1258d4f06178b1` | `605f56921635606f9ec67f0b5f61aebe6dfece94b30bda7f7a25784b16b200f6` | `ad2ba423b697a7d22000c53f4d7961d28397105d0d2e6cc6ab1258d4f06178b1` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/tests/src/utils/za.rs
+++ b/tests/src/utils/za.rs
@@ -1483,14 +1483,15 @@
     for component in &mut ical.components {
         component.entries.retain(|entry| {
             !matches!(
                 entry.name,
                 ICalendarProperty::Dtstamp
                     | ICalendarProperty::Created
                     | ICalendarProperty::LastModified
+                    | ICalendarProperty::Sequence
             )
         });
         for entry in &mut component.entries {
             if entry.name == ICalendarProperty::Uid {
                 entry.values = vec![ICalendarValue::Text(uid.into())];
             } else if entry.name == ICalendarProperty::Organizer {
                 entry
```

## Mutation 46, attempt 1: killed

Required case: 9 (the second run admitted on the attendee side; detection by the direct gate assertion in delegated_attendee_and_alias_edges, ruling R11-46 (a)). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `eec572e014c2f46c4998d7bffae10953ddef9d389a60888160593b8a4a254e80`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 124.999s. Decisive observation:
`sharing.rs:6378` (delegated_attendee_and_alias_edges, block "The attendee side has no second run", fixture `task9-attendee-delegation`): `assertion failed: za_resolve_delegation(&test.server, &sender, &writer, &owner, ZaSelfOperation::Update, Some(&old), Some(&new)).await.unwrap().is_none()`: the public gate admitted a tree whose ORGANIZER is external (key3@example.com) and whose owner is an ATTENDEE. Whole ZA failed 0 passed/1 failed in 60.21s (build 57.57s); last marker `Running direct delegated scheduling producer and lifecycle fixtures...`. Case 25, case 9 (sharing_http_schedule_contracts, including `case9-attendee-reply`) and case 10 passed under the mutant before it.

State evidence: No scope; the audit's corrected definition with its traces (default switch, the mutant). `delegated_organizer` (the gate shared by `za_resolve_delegation` and the builders) also admits a tree with an owner-local ATTENDEE, and `za_self_update` dispatches as upstream's `itip_update` does under owner locality: `attendee_handle_update` when the old organizer is not owner-local, `CannotModifyAddress` for the attendee-side fallback. HTTP trace evidence for `case9-attendee-reply` (Bob's attendee-only writes as Alice on the held external invitation): ACCEPTED write: `attendee-side second run: built=[(false, 1)]`, `retained=0`, `observe pre: update=true, resolution=true, second=Some(0)`, `observe post: statuses=[(true, "1.2")], schedule_tag=Some(2)`; DELEGATED write: `built=[(false, 1)]`, `retained=0`, `second=Some(0)`, `statuses=[(true, "1.2")], schedule_tag=Some(3)`. The tag before the ACCEPTED write was `Some(1)`, derived: `update=true` means a tag was present, and the mutant takes `second.is_some()` as organizer with `first_built` true, so the tag rose by one per write (1 to 2, then 2 to 3). Under the original, by code reading (no observe-only twin was run), the gate refuses, `second` is None, the first run's REPLY is not `from_organizer`, so the tag stays `Some(1)` across both writes. Built and retained counts and status lists are the same as the original's by construction (the retained second-run set is empty; the single REPLY to Carol comes from the first run); the fixture's own counts held (one REPLY in Carol's inbox per write, Bob's inbox unchanged). No other HTTP write reached the attendee-side builder (two attendee-side traces in the log); 0 `ItipMessageError` lines. Restored update.rs and delegation.rs SHA256 equal pre; the working-tree status matches pre-status.txt.

Ruling: Killed by the direct API-level assertion on the exact gate rule (ruling R11-46 (a)), recorded as such: the run failed at the attendee-side gate assertion with exactly the state the condition explains. HTTP observability: at case 9 the mutant is not equivalent; the held event's stored schedule tag increments on attendee-only writes (1 to 2 to 3) where the original keeps it (RFC 6638: attendee-only changes keep the Schedule-Tag), while messages, retained counts and SCHEDULE-STATUS lists do not differ. No HTTP oracle reads that tag, so case 9 alone does not detect the row; per R11-46 (b) the coordinator decides on the proposed test-only oracle (stored schedule_tag unchanged across `case9-attendee-reply`'s ACCEPTED and DELEGATED writes). Not added here. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `383e2523ede985090bb35f9043c49ac66161235763aa5832a937517e4e53e81b` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `eb7d6cae5aa7dbd67a7daf78113b774e967d3d976203cda8e82cc66eb1e31d5e` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -326,14 +326,23 @@
                             event_account,
                             resolution,
                         )
                     }
                     .ok()
                     .flatten()
                 });
+                if writer_account.account_id != event_account.account_id {
+                    eprintln!(
+                        "Mutation46 observe pre: uid={:?}, update={}, resolution={}, second={:?}",
+                        new_event.data.event.uids().next(),
+                        is_update,
+                        za_resolution.is_some(),
+                        second.as_ref().map(|second| second.messages().len())
+                    );
+                }
                 let result = if is_update {
                     itip_update(
                         &mut new_event.data.event,
                         &old_ical,
                         account_info.addresses(),
                     )
                 } else {
@@ -461,14 +470,26 @@
                             ZaSource { document_id, uid },
                             ZaFanOutKind::Change,
                             None,
                             za_resolution.as_ref(),
                         )
                         .await
                         .caused_by(trc::location!())?;
+                    if writer_account.account_id != event_account.account_id {
+                        eprintln!(
+                            "Mutation46 observe post: uid={}, statuses={:?}, schedule_tag={:?}",
+                            uid,
+                            fan_out
+                                .statuses
+                                .iter()
+                                .map(|status| (status.on_organizer, status.status))
+                                .collect::<Vec<_>>(),
+                            new_event.schedule_tag
+                        );
+                    }
                     za_set_schedule_status(&mut new_event.data.event, &fan_out.statuses);
                     za_set_sequences(&mut new_event.data.event, &fan_out.sequences);
                     pairs = fan_out.pairs;
                 }
                 seal_event_in_domain(&mut new_event, &contexts[0].domain, pairs.as_ref())
                     .map_err(|err| seal_error(err, account_id, document_id))?;
             }
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -209,21 +209,24 @@
 ) -> bool {
     writer.account_id != owner.account_id
         && resolution.writer_id == writer.account_id
         && resolution.owner_id == owner.account_id
         && tree.components.iter().any(|component| {
             component.component_type.is_scheduling_object()
                 && component.entries.iter().any(|entry| {
-                    entry.name == ICalendarProperty::Organizer
-                        && entry
-                            .values
-                            .first()
-                            .and_then(|value| value.as_text())
-                            .and_then(|address| Email::new(address, &owner.addresses))
-                            .is_some_and(|email| email.is_local)
+                    // Mutation46: an owner-local ATTENDEE also passes (attendee side).
+                    matches!(
+                        entry.name,
+                        ICalendarProperty::Organizer | ICalendarProperty::Attendee
+                    ) && entry
+                        .values
+                        .first()
+                        .and_then(|value| value.as_text())
+                        .and_then(|address| Email::new(address, &owner.addresses))
+                        .is_some_and(|email| email.is_local)
                 })
         })
 }

 static FORCE_REQUEST: ICalendarScheduleForceSendValue = ICalendarScheduleForceSendValue::Request;

 fn owner_snapshot<'a>(
@@ -308,24 +311,59 @@
     }
     let old_snapshot = owner_snapshot(old, owner, resolution)?;
     match owner_snapshot(new, owner, resolution) {
         Ok(new_snapshot) => {
             if old_snapshot.organizer.email != new_snapshot.organizer.email {
                 return Err(ItipError::OrganizerMismatch);
             }
+            if !old_snapshot.organizer.email.is_local {
+                // Mutation46: upstream's attendee branch under owner locality.
+                let built = crate::scheduling::attendee::attendee_handle_update(
+                    new,
+                    old_snapshot,
+                    new_snapshot,
+                );
+                match &built {
+                    Ok(messages) => eprintln!(
+                        "Mutation46 attendee-side second run: built={:?}",
+                        messages
+                            .iter()
+                            .map(|message| (message.from_organizer, message.to.len()))
+                            .collect::<Vec<_>>()
+                    ),
+                    Err(err) => eprintln!("Mutation46 attendee-side second run: error={err:?}"),
+                }
+                return built.map(|messages| {
+                    let retained = retain_writer_messages(messages, vec![], true, resolution);
+                    eprintln!(
+                        "Mutation46 attendee-side second run: retained={}",
+                        retained.messages.len()
+                    );
+                    Some(retained)
+                });
+            }
             let mut increments = Vec::new();
             organizer_handle_update(old, new, old_snapshot, new_snapshot, &mut increments).map(
                 |messages| {
                     Some(retain_writer_messages(
                         messages, increments, true, resolution,
                     ))
                 },
             )
         }
         Err(err) => match err {
+            // Mutation46: upstream's attendee-side fallback.
+            ItipError::NoSchedulingInfo
+            | ItipError::NotOrganizer
+            | ItipError::NotOrganizerNorAttendee
+            | ItipError::OtherSchedulingAgent
+                if !old_snapshot.organizer.email.is_local =>
+            {
+                Err(ItipError::CannotModifyAddress)
+            }
             ItipError::NoSchedulingInfo
             | ItipError::NotOrganizer
             | ItipError::NotOrganizerNorAttendee
             | ItipError::OtherSchedulingAgent => {
                 itip_cancel_from_snapshot(old, old_snapshot, false).map(|message| {
                     Some(retain_writer_messages(
                         vec![message],
```

## Mutation 46, attempt 2: killed

Required case: 9 (unit supplement: groupware calendar::delegation::tests::second_run_uses_owner_locality, same patch as attempt 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `eec572e014c2f46c4998d7bffae10953ddef9d389a60888160593b8a4a254e80`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test -p groupware calendar::delegation::tests::second_run_uses_owner_locality -- --nocapture
```

Exit `101`, elapsed wall time 8.571s. Decisive observation:
`crates/groupware/src/calendar/delegation.rs:579` in the mutated file (the `attendee_side` assertion, `za_self_update(&attendee_side, &attendee_side, ...).unwrap().is_none()`, 540-542 at HEAD): `called Result::unwrap() on an Err value: NothingToSend`, preceded by the trace `Mutation46 attendee-side second run: error=NothingToSend`. Unit result 0 passed/1 failed (145 filtered out); build 4.72s.

State evidence: Same patch as attempt 1 (identical mutation.patch; its update.rs traces are not compiled into the groupware unit). The `attendee_side` tree (ORGANIZER external, ATTENDEEs owner and writer) now passes the gate, and the attendee-side builder on an unchanged tree returns NothingToSend instead of the original `Ok(None)`.

Ruling: Supplementary direct API kill on the exact gate rule; attempt 1 holds the za-suite detection (the gate assertion in delegated_attendee_and_alias_edges) and the case 9 trace evidence. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `383e2523ede985090bb35f9043c49ac66161235763aa5832a937517e4e53e81b` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `eb7d6cae5aa7dbd67a7daf78113b774e967d3d976203cda8e82cc66eb1e31d5e` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -326,14 +326,23 @@
                             event_account,
                             resolution,
                         )
                     }
                     .ok()
                     .flatten()
                 });
+                if writer_account.account_id != event_account.account_id {
+                    eprintln!(
+                        "Mutation46 observe pre: uid={:?}, update={}, resolution={}, second={:?}",
+                        new_event.data.event.uids().next(),
+                        is_update,
+                        za_resolution.is_some(),
+                        second.as_ref().map(|second| second.messages().len())
+                    );
+                }
                 let result = if is_update {
                     itip_update(
                         &mut new_event.data.event,
                         &old_ical,
                         account_info.addresses(),
                     )
                 } else {
@@ -461,14 +470,26 @@
                             ZaSource { document_id, uid },
                             ZaFanOutKind::Change,
                             None,
                             za_resolution.as_ref(),
                         )
                         .await
                         .caused_by(trc::location!())?;
+                    if writer_account.account_id != event_account.account_id {
+                        eprintln!(
+                            "Mutation46 observe post: uid={}, statuses={:?}, schedule_tag={:?}",
+                            uid,
+                            fan_out
+                                .statuses
+                                .iter()
+                                .map(|status| (status.on_organizer, status.status))
+                                .collect::<Vec<_>>(),
+                            new_event.schedule_tag
+                        );
+                    }
                     za_set_schedule_status(&mut new_event.data.event, &fan_out.statuses);
                     za_set_sequences(&mut new_event.data.event, &fan_out.sequences);
                     pairs = fan_out.pairs;
                 }
                 seal_event_in_domain(&mut new_event, &contexts[0].domain, pairs.as_ref())
                     .map_err(|err| seal_error(err, account_id, document_id))?;
             }
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -209,21 +209,24 @@
 ) -> bool {
     writer.account_id != owner.account_id
         && resolution.writer_id == writer.account_id
         && resolution.owner_id == owner.account_id
         && tree.components.iter().any(|component| {
             component.component_type.is_scheduling_object()
                 && component.entries.iter().any(|entry| {
-                    entry.name == ICalendarProperty::Organizer
-                        && entry
-                            .values
-                            .first()
-                            .and_then(|value| value.as_text())
-                            .and_then(|address| Email::new(address, &owner.addresses))
-                            .is_some_and(|email| email.is_local)
+                    // Mutation46: an owner-local ATTENDEE also passes (attendee side).
+                    matches!(
+                        entry.name,
+                        ICalendarProperty::Organizer | ICalendarProperty::Attendee
+                    ) && entry
+                        .values
+                        .first()
+                        .and_then(|value| value.as_text())
+                        .and_then(|address| Email::new(address, &owner.addresses))
+                        .is_some_and(|email| email.is_local)
                 })
         })
 }

 static FORCE_REQUEST: ICalendarScheduleForceSendValue = ICalendarScheduleForceSendValue::Request;

 fn owner_snapshot<'a>(
@@ -308,24 +311,59 @@
     }
     let old_snapshot = owner_snapshot(old, owner, resolution)?;
     match owner_snapshot(new, owner, resolution) {
         Ok(new_snapshot) => {
             if old_snapshot.organizer.email != new_snapshot.organizer.email {
                 return Err(ItipError::OrganizerMismatch);
             }
+            if !old_snapshot.organizer.email.is_local {
+                // Mutation46: upstream's attendee branch under owner locality.
+                let built = crate::scheduling::attendee::attendee_handle_update(
+                    new,
+                    old_snapshot,
+                    new_snapshot,
+                );
+                match &built {
+                    Ok(messages) => eprintln!(
+                        "Mutation46 attendee-side second run: built={:?}",
+                        messages
+                            .iter()
+                            .map(|message| (message.from_organizer, message.to.len()))
+                            .collect::<Vec<_>>()
+                    ),
+                    Err(err) => eprintln!("Mutation46 attendee-side second run: error={err:?}"),
+                }
+                return built.map(|messages| {
+                    let retained = retain_writer_messages(messages, vec![], true, resolution);
+                    eprintln!(
+                        "Mutation46 attendee-side second run: retained={}",
+                        retained.messages.len()
+                    );
+                    Some(retained)
+                });
+            }
             let mut increments = Vec::new();
             organizer_handle_update(old, new, old_snapshot, new_snapshot, &mut increments).map(
                 |messages| {
                     Some(retain_writer_messages(
                         messages, increments, true, resolution,
                     ))
                 },
             )
         }
         Err(err) => match err {
+            // Mutation46: upstream's attendee-side fallback.
+            ItipError::NoSchedulingInfo
+            | ItipError::NotOrganizer
+            | ItipError::NotOrganizerNorAttendee
+            | ItipError::OtherSchedulingAgent
+                if !old_snapshot.organizer.email.is_local =>
+            {
+                Err(ItipError::CannotModifyAddress)
+            }
             ItipError::NoSchedulingInfo
             | ItipError::NotOrganizer
             | ItipError::NotOrganizerNorAttendee
             | ItipError::OtherSchedulingAgent => {
                 itip_cancel_from_snapshot(old, old_snapshot, false).map(|message| {
                     Some(retain_writer_messages(
                         vec![message],
```

## Mutation 47, attempt 1: killed

Required case: 25 CLIENT/NONE (scheduling agent overridden; case 25 agent-CLIENT differential). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `b222ecb793049c65c4f1ab9219de02bcca4847e3b753d7d680aae977787714c9`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 113.055s. Decisive observation:
tests/src/za/sharing.rs:7726 (sharing_compare_lifecycle), assertion `left == right` failed: "agent-CLIENT step0: inbox payload/address multisets"; left (owner run) = [] (no inbox item), right (delegated run) = one REQUEST (SUMMARY agent, UID same-uid, ATTENDEE sharing-life-bob@example.com, SEQUENCE 0) delivered to [sharing-life-bob@example.com].

State evidence: Run reached 'Running case25 HTTP owner/delegated lifecycle differential' after the earlier modules passed and failed 50.12 s into the suite at the agent-CLIENT label, step 0, the first comparison of that label; matches the predicted first failure. Definition = audit row 47 (attendee.is_server_scheduling = true after the force_send/rsvp reset in the delegated writer path); printed patch changed lines identical to audit patch mutation-47.patch.

Ruling: Killed at case 25 CLIENT/NONE, the first of the row's required cases: the condition makes the delegated writer's CLIENT-scheduled attendee receive a server REQUEST that the owner run does not produce, and the differential assertion states exactly that. Not a compile, setup, timeout or cleanup failure; no deviation from the prediction.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `36a6635997a6468a216dff1d20e51ab343ca70d32406d04addb928e0ce794cc9` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -241,14 +241,16 @@
             .map(|mut attendee| {
                 if !attendee.email.is_local
                     && za_normalize_address(&attendee.email.email)
                         .is_some_and(|key| resolution.override_aliases.contains(&key))
                 {
                     attendee.force_send = Some(&FORCE_REQUEST);
                     attendee.rsvp = None;
+                    // Mutation47: scheduling agent overridden.
+                    attendee.is_server_scheduling = true;
                 }
                 attendee
             })
             .collect();
     }
     Ok(snapshot)
 }
```

## Mutation 47, attempt 2: killed

Required case: 25 CLIENT/NONE (unit supplement: groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched, same patch as attempt 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `b222ecb793049c65c4f1ab9219de02bcca4847e3b753d7d680aae977787714c9`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test -p groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched -- --nocapture
```

Exit `101`, elapsed wall time 8.619s. Decisive observation:
crates/groupware/src/calendar/delegation.rs:697 (unit snapshot_exception_leaves_scheduling_agent_and_payload_untouched): assertion failed: matches!(za_self_create(&client, &writer, &owner, &res), Err(ItipError::NothingToSend)) for the CLIENT tree. Unit result 0 passed, 1 failed (145 filtered out).

State evidence: MUTATION_UNIT=1 prepare; the printed patch is the same two added lines as attempt 1 (is_server_scheduling forced true), changed lines identical to the audit's mutation-47-mutation_unit.patch. The CLIENT-scheduled attendee is no longer left untouched, so the builder returns something other than NothingToSend. Elapsed about 8.6 s including build.

Ruling: Supplementary unit kill on the exact snapshot rule; attempt 1 holds the required case 25 HTTP evidence, which this unit does not replace. Source restored to its exact pre-mutation bytes. (Housekeeping: a stray never-executed, unreviewed prepare of attempt 2 without MUTATION_UNIT was created by mistake and its empty directory deleted before this attempt was prepared.)

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `36a6635997a6468a216dff1d20e51ab343ca70d32406d04addb928e0ce794cc9` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -241,14 +241,16 @@
             .map(|mut attendee| {
                 if !attendee.email.is_local
                     && za_normalize_address(&attendee.email.email)
                         .is_some_and(|key| resolution.override_aliases.contains(&key))
                 {
                     attendee.force_send = Some(&FORCE_REQUEST);
                     attendee.rsvp = None;
+                    // Mutation47: scheduling agent overridden.
+                    attendee.is_server_scheduling = true;
                 }
                 attendee
             })
             .collect();
     }
     Ok(snapshot)
 }
```

## Mutation 48, attempt 1: killed

Required case: 25 sole ForceSend and 10 (reconciliation without finalize); killed at case 25 force-sole, the first of the row's two required cases. Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `7b9273661acf125e60d82413278159cd045880ed0a2e281eabee0c5c90d02fcd`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 96.474s. Decisive observation:
tests/src/za/sharing.rs:7705 (sharing_compare_lifecycle, `label.contains("force") || label.contains("sole-group")` source check): assertion failed: !parsed.components.iter().flat_map(|component| &component.entries).flat_map(|entry| &entry.params).any(|parameter| parameter.name == ICalendarParameterName::ScheduleForceSend), i.e. the stored source still carries SCHEDULE-FORCE-SEND (the preceding `raw.schedule_tag.is_some()` assertion at the same block passed).

State evidence: Run reached 'Running case25 HTTP owner/delegated lifecycle differential...' and failed 48.12 s into the suite. The agent-CLIENT and agent-NONE labels (which precede force-sole and contain neither 'force' nor 'sole-group') passed; force-sole is the first label that enters the source check, so the failure is its step 0. Definition = audit row 48 (both PUT branches in crates/dav/src/calendar/update.rs lose the za_finalize call for the second-run-only case); printed patch's changed lines identical to audit patch mutation-48.patch. The panic message carries no label or run index: the run is attributed to the delegated run because only a delegated PUT has a second run, so the omitted finalize is reachable only there (inference from the mutation's reach; the schedule-tag assertion passing and the ForceSend assertion failing is what the omitted finalize would produce).

Historical ruling (case10 gap later closed by attempt2): Killed at case25 force-sole, the first of the row's two required cases; case 10's half is insensitive by construction (case10-sole-delegated carries no SCHEDULE-FORCE-SEND and no DTSTART edit), per ruling R11-48, and it is flagged for the original chat as a fixture gap. No case-10 attempt was made (ruling R11-48). No deviation from the prediction.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `1f02768d531cecad24481c489042575f12b533a5246d0789494bce82f314966d` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -367,15 +367,15 @@
                     }
                 };
                 if first_built || second.is_some() {
                     if first_nothing
                         && let Some(second) = &second
                         && second.finalize()
                     {
-                        za_finalize(&mut new_event.data.event, second.increments());
+                        // Mutation48 omits second-only finalize.
                     }
                     let is_organizer = second.is_some()
                         || first.as_ref().is_some_and(|messages| {
                             messages.iter().any(|message| message.from_organizer)
                         });
                     if is_organizer {
                         if first_built {
@@ -672,15 +672,15 @@
                     }
                 };
                 if first_built || second.is_some() {
                     if first_nothing
                         && let Some(second) = &second
                         && second.finalize()
                     {
-                        za_finalize(&mut event.data.event, second.increments());
+                        // Mutation48 omits second-only finalize.
                     }
                     event.schedule_tag = Some(1);
                     if itip_status.is_send_sealed() {
                         let messages = if let Some(resolution) = &za_resolution {
                             za_combine_delegated(first, second, resolution)
                         } else {
                             first.into_iter().map(ZaFanOutMessage::upstream).collect()
```

## Mutation 49, attempt 1: killed

Required case: 25 sub-address item counts/tokens (case 25 subaddress label, step 0). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `82e85052463671fa4a7c98aa952c0b6641b82f5cab8856cefac809346f6d3b62`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 95.242s. Decisive observation:
tests/src/za/sharing.rs:7726 (sharing_compare_lifecycle): assertion `left == right` failed: "subaddress step0: inbox payload/address multisets"; left (owner run) holds two METHOD:REQUEST items and right (delegated run) holds one. Every item's address list is the same three addresses [sharing-life-bob+x@example.com, sharing-life-bob@example.com, sharing-life-carol@example.com]; right's single item equals left's first item over the first 3000 characters, so the delegated run lacks one of the owner run's two deliveries.

State evidence: Run reached 'Running case25 HTTP owner/delegated lifecycle differential...' and failed 48.38 s into the suite at the subaddress label, step 0, as predicted; the earlier labels (eleven, listed-alias) passed. Definition = audit row 49 (the lookup branch of za_resolve no longer inserts the key into resolution.recipients when its account id is the writer's, so a writer sub-address is judged by listed-address membership only); printed patch's changed lines identical to audit patch mutation-49.patch. Observation detail: the prediction worded the difference as 'the delegated run does not deliver sharing-life-bob+x'; the log shows it as a missing second item (count 2 versus 1) with identical address lists on the items present, which is the item-count difference the ledger names ('sub-address item counts').

Ruling: Killed at case 25 subaddress, the row's required case: the condition changes the delegated run's delivered item multiset relative to the owner run, which is exactly what the differential asserts. Not a compile, setup, timeout or cleanup failure. No deviation from the predicted label and step.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `541c075faf58e5dc487211deccc896aaa30a1a2016ab7dbe840a0cc05402b783` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -140,17 +140,15 @@
                 let result = if fail {
                     Err(trc::StoreEvent::UnexpectedError.into_err())
                 } else {
                     server.za_account_id_from_email(&key).await
                 };
                 let result = match result {
                     Ok(Some(id)) => {
-                        if id == writer_account.account_id {
-                            resolution.recipients.insert(key.clone());
-                        }
+                        // Mutation49 uses listed-address membership only.
                         ZaResolvedAccount::Account(id)
                     }
                     Ok(None) => ZaResolvedAccount::NotFound,
                     Err(err) => {
                         trc::error!(
                             err.account_id(writer_account.account_id)
                                 .details("zero-access: fan-out recipient lookup failed")
```

## Mutation 50, attempt 1: killed

Required case: 25 owner's stored per-line SCHEDULE-STATUS (case 25 subaddress label, step 0). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `1d10b96d5024fef9dde36815f04bc9651496ac13ee49d2cb3cc7df8c7e50d931`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 95.021s. Decisive observation:
tests/src/za/sharing.rs:7734 (sharing_compare_lifecycle): assertion `left == right` failed: "subaddress step0: owner-stored recipient statuses"; left (owner run) = [(bob, [1.2, 1.2]), (work, []), (bob+x, [1.2]), (carol, [1.2, 1.2])], right (delegated run) = same except (sharing-life-bob+x@example.com, [5.3]).

State evidence: Run reached case 25 and failed 47.82 s into the suite at the subaddress label, step 0, one assertion after the inbox multiset comparison (7726) passed, as predicted (the first-run output now keeps the writer's own addresses in message.to, so the owner's stored per-line status for the writer sub-address becomes 5.3 instead of 1.2). Definition = audit row 50 (the first-run recipient retain in za_combine_delegated is removed); printed patch's changed lines identical to audit patch mutation-50.patch. Compile warnings from the removed use were expected and are not a deviation.

Ruling: Killed at case 25 subaddress, the row's required case, by the per-line SCHEDULE-STATUS assertion the ledger names; the condition (first-run writer recipients retained) explains the changed status for the writer's sub-address. Not a compile, setup, timeout or cleanup failure. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `6d42bb1cb8d1fe098e3d6ee57c6c9d3541551059ce5e76590a5cabfe12a6cab1` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -172,18 +172,15 @@
     first: Vec<ItipMessage<ICalendar>>,
     second: Option<ZaSelfMessages>,
     resolution: &ZaDelegationResolution,
 ) -> Vec<ZaFanOutMessage> {
     let mut combined = first
         .into_iter()
         .filter_map(|mut message| {
-            message.to.retain(|address| {
-                !za_normalize_address(address)
-                    .is_some_and(|key| resolution.recipients.contains(&key))
-            });
+            // Mutation50 retains first-run writer recipients.
             (!message.to.is_empty()).then(|| ZaFanOutMessage::upstream(message))
         })
         .collect::<Vec<_>>();
     if let Some(second) = second {
         combined.extend(
             second
                 .messages
```

## Mutation 51, attempt 1: killed

Required case: Task 9 server-backed negative routing fixture (delegated_scheduling_tests, the `refused` fan-out of upstream-provenance messages with a request-local resolution). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `170eb348279b4ab694fe9dc3b3e94da500ea23a683363d0a8c58f26ac722da3e`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 110.958s. Decisive observation:
tests/src/za/sharing.rs:4835 (delegated_scheduling_tests): assertion `left == right` failed, left `"1.2"`, right `"5.3"`: assert_eq!(refused.statuses[0].status, "5.3") after za_fan_out of public upstream-provenance messages supplied beside a valid resolution.

State evidence: Run reached 'Running direct delegated scheduling producer and lifecycle fixtures...' (the module after case 25's differential, which passed under this mutation) and failed 57.17 s into the suite. With the delegated provenance requirement removed from the self-admission conditions, the writer's own inbox is admitted for a message of upstream provenance and the fan-out reports 1.2 (delivered) where the refusal 5.3 is required. Definition = audit row 51 (both allow_self and Attempt.self_allowed lose the DelegatedSelf provenance test; the ordinary own-account exclusion and the owner exclusion stay); printed patch's changed lines identical to audit patch mutation-51.patch.

Ruling: Killed at the Task 9 negative routing fixture, the row's required case, by the 5.3 refusal assertion that the condition directly breaks. Not a compile, setup, timeout or cleanup failure. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/fanout.rs` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` | `55cb1a1b66ee20278b0a8e87ea635a9b570e763a00505d82d6bce4a96d1fd9ec` | `03cd3b450cccfd47fe9b87f21f03ca78c5149adf9973050a921f0153c1bc9c18` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/fanout.rs
+++ b/crates/groupware/src/calendar/fanout.rs
@@ -415,21 +415,16 @@
                 };
                 if let Some(key) = &key
                     && !recipients.contains_key(key)
                 {
                     let recipient = match resolved.take_if(|(cached, _)| cached == key) {
                         Some((_, recipient)) => recipient,
                         None => {
-                            let allow_self = resolution.is_some()
-                                && messages.iter().any(|candidate| {
-                                    candidate.provenance == ZaMessageProvenance::DelegatedSelf
-                                        && candidate.message.to.iter().any(|address| {
-                                            za_normalize_address(address).as_ref() == Some(key)
-                                        })
-                                });
+                            // Mutation51: self admission by account resolution alone.
+                            let allow_self = resolution.is_some();
                             resolve_recipient(
                                 self,
                                 sender_id,
                                 key,
                                 resolution.and_then(|resolution| resolution.accounts.get(key)),
                                 allow_self,
                                 resolution.map(|_| event_account.account_id),
@@ -444,16 +439,16 @@
                 }
                 attempts.push(Attempt {
                     message,
                     address,
                     is_reply,
                     route,
                     key,
-                    self_allowed: resolution.is_some()
-                        && tagged.provenance == ZaMessageProvenance::DelegatedSelf,
+                    // Mutation51: no provenance requirement.
+                    self_allowed: resolution.is_some(),
                 });
             }
         }
         if read.is_none()
             && (!internal.is_empty()
                 || (!delegated && kind == ZaFanOutKind::Deletion && !messages.is_empty()))
         {
```

## Mutation 52, attempt 1: killed

Required case: Task 9 catch-all trace fixture (catch_all_resolution_is_quiet: R classification and delegated sender verification). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `62f6202786c1b78a4eaef74420cebacf1e09d59f04009cb16fa09325360aefa9`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 112.844s. Decisive observation:
tests/src/za/sharing.rs:5511 (catch_all_resolution_is_quiet): panicked with "quiet classification leaked task9-target-local-canary, cold=true:" followed by the captured lookup trace events (Key=String("task9-target-local-canary") Domain=UInt(7) Collection=String("email"), plus domainId and account lookups).

State evidence: Run reached 'Running direct delegated scheduling producer and lifecycle fixtures...' (after case 25, which passed under this mutation) and failed 61.45 s into the suite, in the cold-cache pass. The catch-all fallback lookup in rcpt_id_from_email (crates/common/src/cache/principals.rs) now passes quiet=false, so the target canary local part is traced by the lookup events. Definition = audit row 52; printed patch's changed lines identical to audit patch mutation-52.patch. Common-crate edit; build and run took 112.8 s in total.

Ruling: Killed at the Task 9 catch-all trace fixture, the row's required case, by the leak assertion the condition (catch-all fallback drops the quiet flag) directly produces. Not a compile, setup, timeout or cleanup failure. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/common/src/cache/principals.rs` | `5cf3c154af147f00a8b679a8d0585224b3597b224a2589bb0ecd07e824f2cf69` | `e73ee5a411ceb8f1e80bc9af77a2195f1b8847bb756094b1207df9c72bbedfa8` | `5cf3c154af147f00a8b679a8d0585224b3597b224a2589bb0ecd07e824f2cf69` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/common/src/cache/principals.rs
+++ b/crates/common/src/cache/principals.rs
@@ -792,15 +792,15 @@
                 let mut result = self
                     .rcpt_id_from_parts_inner(local_part.as_ref(), domain.id, quiet)
                     .await?;
                 if resolve
                     && result.is_none()
                     && let Some(catch_all) = &domain.catch_all
                 {
-                    result = self.rcpt_id_from_email_inner(catch_all, quiet).await?;
+                    result = self.rcpt_id_from_email_inner(catch_all, false).await?;
                 }

                 Ok(result.and_then(|result| {
                     if let EmailCache::Account(account_id) = result {
                         Some(account_id)
                     } else {
                         None
```

## Mutation 53, attempt 1: killed

Required case: 25 group/unlisted/custom/catch-all negative exception (case 25 negative-group-DECLINED). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `fe73bc58ef68097fc9727c3638bf0aab651b55e73c58caa649805e4c2529b565`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 207.501s. Decisive observation:
tests/src/za/sharing.rs:7691 (sharing_compare_lifecycle, `label.starts_with("negative-")` block): panicked with "suppressed negative recipient must have no attempted status".

State evidence: Run reached 'Running case25 HTTP owner/delegated lifecycle differential...' and failed 54.47 s into the suite. The panic message carries no label; the failing label is the first `negative-` label the fixture reaches: the exception-DECLINED and failed-direct-alias-DECLINED labels precede it in the loop and passed, and negative-group-DECLINED is first in the loop order (outer DECLINED, inner list group then unlisted), so the failure is negative-group-DECLINED as predicted. Definition = audit row 53 (the exception's force-send override tests the writer-recipient set R, resolution.recipients, instead of the direct-alias set A, resolution.override_aliases), so a listed group that resolves to the writer receives the override and an attempted status. Printed patch's changed lines identical to audit patch mutation-53.patch. Wall time 207.5 s because of host load (suite portion 54.47 s).

Ruling: Killed at case 25 negative-group-DECLINED, the row's required case, by the 'suppressed negative recipient must have no attempted status' assertion that the condition (exception uses R rather than A) directly breaks. Not a compile, setup, timeout or cleanup failure. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `cd02bbf27b2b3554270fde1253e5874b32f7a8c7aa197203f34424957fdbefdc` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -237,15 +237,15 @@
         // one of its fields while it is inside the set.
         let attendees = std::mem::take(&mut instance.attendees);
         instance.attendees = attendees
             .into_iter()
             .map(|mut attendee| {
                 if !attendee.email.is_local
                     && za_normalize_address(&attendee.email.email)
-                        .is_some_and(|key| resolution.override_aliases.contains(&key))
+                        .is_some_and(|key| resolution.recipients.contains(&key))
                 {
                     attendee.force_send = Some(&FORCE_REQUEST);
                     attendee.rsvp = None;
                 }
                 attendee
             })
             .collect();
```

## Mutation 53, attempt 2: killed

Required case: 25 group/unlisted/custom/catch-all negative exception (unit supplement: groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched, same patch as attempt 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `fe73bc58ef68097fc9727c3638bf0aab651b55e73c58caa649805e4c2529b565`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test -p groupware calendar::delegation::tests::snapshot_exception_leaves_scheduling_agent_and_payload_untouched -- --nocapture
```

Exit `101`, elapsed wall time 21.024s. Decisive observation:
crates/groupware/src/calendar/delegation.rs:622 (unit snapshot_exception_leaves_scheduling_agent_and_payload_untouched): assertion `left == right` failed; left (actual recipient list) = [custom@example.com, writer.alias@example.com, writer@example.com, writers@example.com], right (expected) = [writer.alias@example.com, writer@example.com]. Unit result 0 passed, 1 failed (145 filtered out).

State evidence: MUTATION_UNIT=1 prepare; the printed patch is the same one-line change as attempt 1 (override test uses resolution.recipients instead of resolution.override_aliases), changed lines identical to the audit's mutation-53-mutation_unit.patch. The recipient list gains the group (writers@) and the custom address, as the audit predicted. Elapsed 21.0 s including build.

Ruling: Supplementary unit kill on the exact exception rule; attempt 1 holds the required case 25 HTTP evidence, which this unit does not replace. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `cd02bbf27b2b3554270fde1253e5874b32f7a8c7aa197203f34424957fdbefdc` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -237,15 +237,15 @@
         // one of its fields while it is inside the set.
         let attendees = std::mem::take(&mut instance.attendees);
         instance.attendees = attendees
             .into_iter()
             .map(|mut attendee| {
                 if !attendee.email.is_local
                     && za_normalize_address(&attendee.email.email)
-                        .is_some_and(|key| resolution.override_aliases.contains(&key))
+                        .is_some_and(|key| resolution.recipients.contains(&key))
                 {
                     attendee.force_send = Some(&FORCE_REQUEST);
                     attendee.rsvp = None;
                 }
                 attendee
             })
             .collect();
```

## Mutation 54, attempt 1: killed

Required case: 25 group status and eligible sole-group finalize (case 25 sole-group). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `44a94478797cfb26a876e615411ed012fdf0dadfea69b87b4738a050876cb8ba`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 105.338s. Decisive observation:
tests/src/za/sharing.rs:7668 (sharing_compare_lifecycle, `label.contains("sole-group")` block): assertion failed: !statuses_for(&source_statuses[run], "sharing-life-team@example.com").is_empty(); the preceding `.all(|status| status == "5.3")` assertion passed vacuously on the empty list.

State evidence: Run reached case 25 and failed 54.17 s into the suite. The `sole-group` block is entered only by the sole-group label, which follows eleven, listed-alias, subaddress, subaddress-only, agent-*, force-sole, shared-attendee and shared-organizer; all passed. The delegated run's stored source holds no SCHEDULE-STATUS for the group team@ (the owner run's does), because a listed membership group that resolves to the writer is no longer in R, so it gets no attempted status. The panic message carries no run index; the delegated run is inferred because only it applies the delegated resolution that the mutation changes. Definition = audit row 54 (resolution.recipients.insert for a listed address made conditional on aliases.contains(&key)); printed patch's changed lines identical to audit patch mutation-54.patch.

Ruling: Killed at case 25 sole-group, the row's required case, by the group-status assertion that the condition (writer-only group omitted from R) directly breaks. Not a compile, setup, timeout or cleanup failure. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `7176a5dfb6dc3f8640715cce54ee1450caf0e5bb1e74580b26ca10dfe4e86691` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -118,15 +118,18 @@
                     continue;
                 }
                 let Some(key) = za_normalize_address(&email.email) else {
                     continue;
                 };
                 if sender.addresses.contains(&email.email) {
                     resolution.listed.insert(key.clone());
-                    resolution.recipients.insert(key.clone());
+                    // Mutation54: a listed membership group is omitted from R.
+                    if aliases.contains(&key) {
+                        resolution.recipients.insert(key.clone());
+                    }
                     if aliases.contains(&key) {
                         resolution.override_aliases.insert(key.clone());
                     }
                 }
                 if resolution.accounts.contains_key(&key) {
                     continue;
                 }
```

## Mutation 55, attempt 1: killed

Required case: 12 actual outer delete-result Option seam in owner/Bob runs (copy_move_test: MOVE overwrite must not build a CANCEL). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `c1b26e173b0396db5bf7a65b4885a450c7b82b90473c12c498e1114154223558`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 125.938s. Decisive observation:
tests/src/za/sharing.rs:834 (copy_move_test, starts at line 527): assertion `left == right` failed: "MOVE overwrite must not build a CANCEL"; left Some(true), right Some(false) (overwrite_observation, the outer delete-result Option recorded by record_move_overwrite_message).

State evidence: Run reached 'Running conversion assertion, rollback, resume and cache contracts...' (copy_move_test's module; the log's last Running marker) and failed 71.50 s into the suite, before the whole-suite tail; the earlier assertions of copy_move_test (counts unchanged, attendee copy active) passed and its 'Task5 overwrite observation=...' line precedes the failure. Definition = audit row 55 (MOVE overwrite passes schedule=true to delete_za_at in crates/dav/src/calendar/copy_move.rs); printed patch's changed lines identical to audit patch mutation-55.patch. The prediction's note that the run takes ~340 s plus build did not apply: the failing module ran earlier in the suite than assumed, which is not a deviation in label or assertion. (The one pattern hit in run.log, a 'Mutation34 oracle' line, is the baseline oracle's own diagnostic print, unrelated to row 55.)

Ruling: Killed at case 12 (copy_move_test), the row's required case, by the outer delete-result Option assertion the ledger names: Some(true) where Some(false) is required. Not a compile, setup, timeout or cleanup failure. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/copy_move.rs` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` | `4984d88824e383276e5fee6e0de62bcbb2cfc173f1eebeb6886b1532a8bed26e` | `c6635c40c48d5e705011078da792934827efa6a1ab526810e7340a183dbcc103` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/copy_move.rs
+++ b/crates/dav/src/calendar/copy_move.rs
@@ -1567,15 +1567,15 @@
             }
             let result = DestroyArchive(stored.to_unarchived::<CalendarEvent>()?).delete_za_at(
                 &account,
                 owner,
                 id,
                 target,
                 None,
-                false,
+                true,
                 Some(keys),
                 Some(to),
                 cutoff,
                 &mut batch,
             )?;
             #[cfg(feature = "test_mode")]
             common::za::test_hooks::record_move_overwrite_message(owner, id, result.is_some());
```

## Mutation 56, attempt 1: killed

Required case: 25 failed direct-alias exception: Run B 5.1 and no item; Run A suppressed (case 25 failed-direct-alias-DECLINED). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `e2ad21f1dfd102bd9faa4396189135d19ff0cebe1de571b781693f4b57977fef`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 95.517s. Decisive observation:
tests/src/za/sharing.rs:7680 (sharing_compare_lifecycle, failed-direct-alias block): assertion `left == right` failed; left [] (statuses_for(&source_statuses[run], LIFE_ALIAS)), right ["5.1"] (the required status for the delegated run, whose forced recipient lookup fails).

State evidence: Run reached case 25 and failed 53.24 s into the suite. The panic message carries no label; failed-direct-alias-DECLINED is the first failed-direct-alias label (it follows exception-DECLINED in the DECLINED iteration, which passed), and the block's expected value is the empty list for run 0 and ["5.1"] otherwise, so the failing run is the delegated one, which now gets no status for the alias. Definition = audit row 56 (A, resolution.override_aliases, is derived only from a successful lookup resolving to the writer, no longer before the lookup; with the injected lookup failure the alias drops out of A and receives no 5.1 attempt); printed patch's changed lines identical to audit patch mutation-56.patch.

Ruling: Killed at case 25 failed-direct-alias-DECLINED, the row's required case, by the 5.1 status assertion that the condition (A derived only from a successful recipient lookup) directly breaks. Not a compile, setup, timeout or cleanup failure. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `66d3f1c744677e8f9343458824ee3441ede57fc179431b5824801fcc12f52ce6` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -119,17 +119,15 @@
                 }
                 let Some(key) = za_normalize_address(&email.email) else {
                     continue;
                 };
                 if sender.addresses.contains(&email.email) {
                     resolution.listed.insert(key.clone());
                     resolution.recipients.insert(key.clone());
-                    if aliases.contains(&key) {
-                        resolution.override_aliases.insert(key.clone());
-                    }
+                    // Mutation56: A is no longer derived before the lookup.
                 }
                 if resolution.accounts.contains_key(&key) {
                     continue;
                 }
                 #[cfg(feature = "test_mode")]
                 let fail = common::za::test_hooks::take_delegation_lookup_fail_once(
                     writer_account.account_id,
@@ -142,14 +140,18 @@
                 } else {
                     server.za_account_id_from_email(&key).await
                 };
                 let result = match result {
                     Ok(Some(id)) => {
                         if id == writer_account.account_id {
                             resolution.recipients.insert(key.clone());
+                            // Mutation56: A derived only from a successful lookup.
+                            if resolution.listed.contains(&key) && aliases.contains(&key) {
+                                resolution.override_aliases.insert(key.clone());
+                            }
                         }
                         ZaResolvedAccount::Account(id)
                     }
                     Ok(None) => ZaResolvedAccount::NotFound,
                     Err(err) => {
                         trc::error!(
                             err.account_id(writer_account.account_id)
```

## Mutation 57, attempt 1: killed

Required case: 2 and section 11.1 (case 2: a grantee's dead-property write must not overwrite the owner's personal bundle; grantee_dav_admission). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `f2695a3b7f98d05bd09766247d6d976b6b87eced2effbb1ce3f4c0a14dbe13ba`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 98.843s. Decisive observation:
tests/src/za/sharing.rs:6965 (grantee_dav_admission, starts at line 6781): assertion failed: owner_after.contains("Shared owner private name"). It follows Bob's PROPPATCH (207 Multi-Status, accepted) and the owner's own PROPFIND (also 207); `owner_before.contains("Shared owner private name")` at 6964 passed, so the owner's personal displayname is lost by Bob's dead-property write.

State evidence: Run reached 'Running Task 10 grantee DAV admission and method matrix...' and failed 43.77 s into the suite. The mutation trace line precedes the failing check in run.log (line 53 'Mutation57 grantee dead-property write sealed into the owner personal bundle: personal_id=17, owner=16', failure at line 55). There was no 500 and no SealError: both PROPFIND requests and Bob's PROPPATCH returned 207, and the failure is the content assertion, so the cause is the stored state the definition produces: the owner's entry parts 1-2 (ck-bundle envelope and ciphertext) rewritten with a bundle whose name is empty and which holds the dead properties, parts 3-4 (shared envelope and bundle) unchanged. Definition = audit row 57 (Bob, personal != owner, takes the new branch in crates/groupware/src/calendar/seal/collection.rs); printed patch's changed lines identical to audit patch mutation-57.patch. The owner's PROPFIND no longer shows the owner's private name, which is exactly the ledger condition.

Ruling: Killed at case 2 (grantee_dav_admission), the row's required case, by a content assertion that the condition (grantee dead property overwrites the owner's personal bundle) directly produces; not a generic 500, compile, setup, timeout or cleanup failure, so ruling M9's causal-origin requirement is met by the trace line preceding the failure and the 207 responses. Predicted failure mode was a 500 at the owner PROPFIND or this name check; the name check is the one that occurred. No deviation.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/seal/collection.rs` | `ba8629d92e82c0aeae2cdbd67f405a7dc860d93d2419167626fccb49582c970b` | `ac068a461d749ff9338bb545e92cb8fb432e432a58f8b637fd9b7ec59ad80092` | `ba8629d92e82c0aeae2cdbd67f405a7dc860d93d2419167626fccb49582c970b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/seal/collection.rs
+++ b/crates/groupware/src/calendar/seal/collection.rs
@@ -475,15 +475,47 @@
         target.sort_order = edited_personal.sort_order;
         target.flags = edited_personal.flags;
         target.default_alerts = edited_personal.default_alerts.clone();
     }
     if edited_personal.color.is_some() {
         return Err(SealError::Structure("personal color"));
     }
-    if edited.dead_properties != opened.dead_properties {
+    if edited.dead_properties != opened.dead_properties && personal != owner {
+        // Mutation57: a grantee's dead properties sealed into the owner's personal
+        // bundle (parts 1-2) instead of the shared bundle (parts 3-4).
+        let owner_id = domain.owner_id();
+        let principal = owner_id.to_be_bytes();
+        let dek = Secret::random();
+        let bundle = Bundle {
+            name: String::new(),
+            description: None,
+            color: None,
+            dead_properties: edited.dead_properties.clone(),
+        };
+        let plain =
+            rkyv::to_bytes::<rkyv::rancor::Error>(&bundle).map_err(|_| SealError::Format)?;
+        let envelope = domain.seal_ck(&dek, "ck-bundle", Some(&principal))?;
+        let sealed = seal_bytes(
+            &dek,
+            &domain.bundle_aad("ck-calendar-bundle", Some(owner_id))?,
+            &plain,
+        );
+        let own = &mut changed.preferences[owner];
+        let owner_parts = entry_parts(own)?;
+        own.name = format!(
+            "{}|{}|{sealed}|{}|{}",
+            COLLECTION_MARKER.to_owned() + owner_parts[0],
+            STANDARD.encode(envelope),
+            owner_parts[3],
+            owner_parts[4]
+        );
+        eprintln!(
+            "Mutation57 grantee dead-property write sealed into the owner personal bundle: personal_id={personal_id}, owner={owner_id}"
+        );
+    } else if edited.dead_properties != opened.dead_properties {
         let (envelope, bundle) = seal_shared_properties(&edited.dead_properties, domain)?;
         let own = &mut changed.preferences[owner];
         let personal_parts = entry_parts(own)?;
         own.name = format!(
             "{}|{}|{}|{envelope}|{bundle}",
             COLLECTION_MARKER.to_owned() + personal_parts[0],
             personal_parts[1],
```

## Mutation 57, attempt 2: killed

Required case: 2 and section 11.1 (unit supplement: groupware calendar::seal::collection::tests::shared_property_write_preserves_owner_parts, same patch as attempt 1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `f2695a3b7f98d05bd09766247d6d976b6b87eced2effbb1ce3f4c0a14dbe13ba`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test -p groupware calendar::seal::collection::tests::shared_property_write_preserves_owner_parts -- --nocapture
```

Exit `101`, elapsed wall time 8.257s. Decisive observation:
crates/groupware/src/calendar/seal/collection.rs:771 at HEAD (reported as :803 in the mutated file, which is 32 lines longer before the test), unit shared_property_write_preserves_owner_parts: assert_eq!(&before[..3], &after[..3]) failed. The first element (the $za$ marker part) is equal on both sides and the second element (the owner entry's ck-bundle envelope) differs: the owner's entry parts 1-2 were rewritten by the grantee's dead-property write. Unit result 0 passed, 1 failed (145 filtered out). (Ciphertext values are test-fixture bytes in the ignored run.log and are not reproduced here.)

State evidence: MUTATION_UNIT=1 prepare; the printed patch is the same as attempt 1, changed lines identical to the audit's mutation-57-mutation_unit.patch. The run.log holds the 'Mutation57 grantee dead-property write sealed into the owner personal bundle: personal_id=8, owner=4' trace line before the assertion. Elapsed 8.3 s including build.

Ruling: Supplementary unit kill on the exact owner-parts rule; attempt 1 holds the required case 2 HTTP evidence, which this unit does not replace. Source restored to its exact pre-mutation bytes.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/groupware/src/calendar/seal/collection.rs` | `ba8629d92e82c0aeae2cdbd67f405a7dc860d93d2419167626fccb49582c970b` | `ac068a461d749ff9338bb545e92cb8fb432e432a58f8b637fd9b7ec59ad80092` | `ba8629d92e82c0aeae2cdbd67f405a7dc860d93d2419167626fccb49582c970b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/groupware/src/calendar/seal/collection.rs
+++ b/crates/groupware/src/calendar/seal/collection.rs
@@ -475,15 +475,47 @@
         target.sort_order = edited_personal.sort_order;
         target.flags = edited_personal.flags;
         target.default_alerts = edited_personal.default_alerts.clone();
     }
     if edited_personal.color.is_some() {
         return Err(SealError::Structure("personal color"));
     }
-    if edited.dead_properties != opened.dead_properties {
+    if edited.dead_properties != opened.dead_properties && personal != owner {
+        // Mutation57: a grantee's dead properties sealed into the owner's personal
+        // bundle (parts 1-2) instead of the shared bundle (parts 3-4).
+        let owner_id = domain.owner_id();
+        let principal = owner_id.to_be_bytes();
+        let dek = Secret::random();
+        let bundle = Bundle {
+            name: String::new(),
+            description: None,
+            color: None,
+            dead_properties: edited.dead_properties.clone(),
+        };
+        let plain =
+            rkyv::to_bytes::<rkyv::rancor::Error>(&bundle).map_err(|_| SealError::Format)?;
+        let envelope = domain.seal_ck(&dek, "ck-bundle", Some(&principal))?;
+        let sealed = seal_bytes(
+            &dek,
+            &domain.bundle_aad("ck-calendar-bundle", Some(owner_id))?,
+            &plain,
+        );
+        let own = &mut changed.preferences[owner];
+        let owner_parts = entry_parts(own)?;
+        own.name = format!(
+            "{}|{}|{sealed}|{}|{}",
+            COLLECTION_MARKER.to_owned() + owner_parts[0],
+            STANDARD.encode(envelope),
+            owner_parts[3],
+            owner_parts[4]
+        );
+        eprintln!(
+            "Mutation57 grantee dead-property write sealed into the owner personal bundle: personal_id={personal_id}, owner={owner_id}"
+        );
+    } else if edited.dead_properties != opened.dead_properties {
         let (envelope, bundle) = seal_shared_properties(&edited.dead_properties, domain)?;
         let own = &mut changed.preferences[owner];
         let personal_parts = entry_parts(own)?;
         own.name = format!(
             "{}|{}|{}|{envelope}|{bundle}",
             COLLECTION_MARKER.to_owned() + personal_parts[0],
             personal_parts[1],
```

## Mutation 46, attempt 3: killed

Required case: 9 (the second run admitted on the attendee side; detection by oracle 46 in sharing_http_schedule_contracts, fixture case9-attendee-reply, ruling R11-46(b)). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `eec572e014c2f46c4998d7bffae10953ddef9d389a60888160593b8a4a254e80`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 116.008s. Decisive observation:
`tests/src/za/sharing.rs:9233:5` (sharing_http_schedule_contracts, fixture `case9-attendee-reply`, patched sharing.rs 6f56132a...): `assertion `left == right` failed: oracle46: Bob's ACCEPTED write as Alice keeps the owner's Schedule-Tag`, left `Some("\"2\"")`, right `Some("\"1\"")`. Whole ZA failed 0 passed/1 failed in 49.87s; no MUTATION_* variables set; patch lines identical to attempt 1's.

State evidence: The Mutation46 traces precede the panic: for case9-delegated and case9-per-component `observe post` shows schedule_tag=Some(2) with second=Some(0) (the ordinary gate-admitted paths); for case9-attendee-reply `attendee-side second run: built=[(false, 1)]`, `retained=0`, then `observe pre: uid=case9-attendee-reply, update=true, resolution=true, second=Some(0)` and `observe post: ... statuses=[(true, "1.2")], schedule_tag=Some(2)`. The held external invitation's stored schedule tag rose from 1 to 2 on Bob's attendee-only ACCEPTED write as Alice, and the served Schedule-Tag followed.

Ruling: Killed at case 9 by an assertion the condition explains. Ruling R11-46(b): the mutant admits the owner-attendee tree to the second run (upstream's attendee branch under owner locality), which retains no message but bumps the stored schedule tag; oracle 46 requires the served Schedule-Tag to be unchanged across Bob's attendee-only ACCEPTED and DELEGATED writes as Alice (RFC 6638 3.2.10), so the mutation is not observationally equivalent on the attendee side. Attempt 1's earlier detection at sharing.rs:6378 (the direct gate assertion, R11-46 (a)) and attempt 2 (the unit supplement) stay recorded; this attempt adds the HTTP-level observable oracle.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `383e2523ede985090bb35f9043c49ac66161235763aa5832a937517e4e53e81b` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |
| `crates/groupware/src/calendar/delegation.rs` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` | `eb7d6cae5aa7dbd67a7daf78113b774e967d3d976203cda8e82cc66eb1e31d5e` | `a47a1d19e9acaef87a64b6822caa485d4bc7b0bfaea759225ae8c4fef02aeecb` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -326,14 +326,23 @@
                             event_account,
                             resolution,
                         )
                     }
                     .ok()
                     .flatten()
                 });
+                if writer_account.account_id != event_account.account_id {
+                    eprintln!(
+                        "Mutation46 observe pre: uid={:?}, update={}, resolution={}, second={:?}",
+                        new_event.data.event.uids().next(),
+                        is_update,
+                        za_resolution.is_some(),
+                        second.as_ref().map(|second| second.messages().len())
+                    );
+                }
                 let result = if is_update {
                     itip_update(
                         &mut new_event.data.event,
                         &old_ical,
                         account_info.addresses(),
                     )
                 } else {
@@ -461,14 +470,26 @@
                             ZaSource { document_id, uid },
                             ZaFanOutKind::Change,
                             None,
                             za_resolution.as_ref(),
                         )
                         .await
                         .caused_by(trc::location!())?;
+                    if writer_account.account_id != event_account.account_id {
+                        eprintln!(
+                            "Mutation46 observe post: uid={}, statuses={:?}, schedule_tag={:?}",
+                            uid,
+                            fan_out
+                                .statuses
+                                .iter()
+                                .map(|status| (status.on_organizer, status.status))
+                                .collect::<Vec<_>>(),
+                            new_event.schedule_tag
+                        );
+                    }
                     za_set_schedule_status(&mut new_event.data.event, &fan_out.statuses);
                     za_set_sequences(&mut new_event.data.event, &fan_out.sequences);
                     pairs = fan_out.pairs;
                 }
                 seal_event_in_domain(&mut new_event, &contexts[0].domain, pairs.as_ref())
                     .map_err(|err| seal_error(err, account_id, document_id))?;
             }
--- a/crates/groupware/src/calendar/delegation.rs
+++ b/crates/groupware/src/calendar/delegation.rs
@@ -209,21 +209,24 @@
 ) -> bool {
     writer.account_id != owner.account_id
         && resolution.writer_id == writer.account_id
         && resolution.owner_id == owner.account_id
         && tree.components.iter().any(|component| {
             component.component_type.is_scheduling_object()
                 && component.entries.iter().any(|entry| {
-                    entry.name == ICalendarProperty::Organizer
-                        && entry
-                            .values
-                            .first()
-                            .and_then(|value| value.as_text())
-                            .and_then(|address| Email::new(address, &owner.addresses))
-                            .is_some_and(|email| email.is_local)
+                    // Mutation46: an owner-local ATTENDEE also passes (attendee side).
+                    matches!(
+                        entry.name,
+                        ICalendarProperty::Organizer | ICalendarProperty::Attendee
+                    ) && entry
+                        .values
+                        .first()
+                        .and_then(|value| value.as_text())
+                        .and_then(|address| Email::new(address, &owner.addresses))
+                        .is_some_and(|email| email.is_local)
                 })
         })
 }

 static FORCE_REQUEST: ICalendarScheduleForceSendValue = ICalendarScheduleForceSendValue::Request;

 fn owner_snapshot<'a>(
@@ -308,24 +311,59 @@
     }
     let old_snapshot = owner_snapshot(old, owner, resolution)?;
     match owner_snapshot(new, owner, resolution) {
         Ok(new_snapshot) => {
             if old_snapshot.organizer.email != new_snapshot.organizer.email {
                 return Err(ItipError::OrganizerMismatch);
             }
+            if !old_snapshot.organizer.email.is_local {
+                // Mutation46: upstream's attendee branch under owner locality.
+                let built = crate::scheduling::attendee::attendee_handle_update(
+                    new,
+                    old_snapshot,
+                    new_snapshot,
+                );
+                match &built {
+                    Ok(messages) => eprintln!(
+                        "Mutation46 attendee-side second run: built={:?}",
+                        messages
+                            .iter()
+                            .map(|message| (message.from_organizer, message.to.len()))
+                            .collect::<Vec<_>>()
+                    ),
+                    Err(err) => eprintln!("Mutation46 attendee-side second run: error={err:?}"),
+                }
+                return built.map(|messages| {
+                    let retained = retain_writer_messages(messages, vec![], true, resolution);
+                    eprintln!(
+                        "Mutation46 attendee-side second run: retained={}",
+                        retained.messages.len()
+                    );
+                    Some(retained)
+                });
+            }
             let mut increments = Vec::new();
             organizer_handle_update(old, new, old_snapshot, new_snapshot, &mut increments).map(
                 |messages| {
                     Some(retain_writer_messages(
                         messages, increments, true, resolution,
                     ))
                 },
             )
         }
         Err(err) => match err {
+            // Mutation46: upstream's attendee-side fallback.
+            ItipError::NoSchedulingInfo
+            | ItipError::NotOrganizer
+            | ItipError::NotOrganizerNorAttendee
+            | ItipError::OtherSchedulingAgent
+                if !old_snapshot.organizer.email.is_local =>
+            {
+                Err(ItipError::CannotModifyAddress)
+            }
             ItipError::NoSchedulingInfo
             | ItipError::NotOrganizer
             | ItipError::NotOrganizerNorAttendee
             | ItipError::OtherSchedulingAgent => {
                 itip_cancel_from_snapshot(old, old_snapshot, false).map(|message| {
                     Some(retain_writer_messages(
                         vec![message],
```

## Mutation 19, attempt 4: killed

Required case: 11 (M6 assisted own-home group-only foreign propname eligibility). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `6d7a6c4fe79e209d0ef0cb88eac87e306554ea6a190efc3d860c1cd05be4d37e`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 233.34s. Decisive observation:
`tests/src/za/sharing.rs:8469`: `case11: assisted propname must omit the group-only foreign calendar`; Grace own-home depth1 propname returned207 containing `/dav/cal/sharing-dav-alice%40example.com/private/` with200 properties. The preceding positive own-default-calendar assertion passed.

State evidence: Whole ZA reached the exact newly added case11 group fixture. Grace belongs to the planted group carrying Read+ReadItems on Alice private calendar, but has no own-id grant. Her direct private request remained403 and foreign-home request remained207. Under this temporary M6 omission the own-home propname response includes the forbidden private href; the unchanged baseline omits it. Earlier corrected corrupt-grant listing accepts request500 or207 with exact corrupt-href item500 and did not cause the failure. Runtime45.34s; total233.34s including compilation. Baseline sharing SHA07053b2c5bfbb5c0cc5512bb71b46f349747385a9ff60c66f462c305134d8ab0.

Ruling: Valid required-case kill under R11-19-review/M6. Removes both physical fresh-eligibility checks only for assisted foreign propname entries on the caller own home: preappend and peritem. Direct/foreign-home eligibility and all archive-view crypto/unseal safeguards remain. This does not rehabilitate attempt3 or claim a single preappend omission alone violates request/item500 packaging. Primary reviewed exact patch before execution. All saved bytes and status independently verified restored before row48.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/common/propfind.rs` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` | `493be2ff117bce3814bd45fb4f16af3e7e8664f96676ffc47fac91e1b1c60ca5` | `2b97c59b550a9c7edc283bf4e086ec1bae79e6a9af3c43f4f900f4fe0547067b` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/common/propfind.rs
+++ b/crates/dav/src/common/propfind.rs
@@ -321,14 +321,19 @@
         let mut is_sync_limited = false;
         let mut is_propfind = false;
         let mut ical_instances_limit = self.core.groupware.max_ical_instances;

         // An otherwise valid home listing omits individual refused calendars.
         // Directly addressed resources and reports retain request-level refusals.
         let za_home_listing = matches!(&query.resource, DavQueryResource::Uri(resource) if resource.resource.is_none());
+        // Mutation19: only assisted entries on the authenticated caller's own home.
+        let mutation19_assisted_own_home = self.core.groupware.assisted_discovery
+            && matches!(&query.resource, DavQueryResource::Uri(resource)
+                if resource.resource.is_none()
+                    && resource.account_id == access_token.account_id());
         let paths = match std::mem::take(&mut query.resource) {
             DavQueryResource::Uri(resource) => {
                 collection_container = resource.collection;
                 collection_children = collection_container.child_collection().unwrap();
                 sync_collection = SyncCollection::from(collection_container);
                 is_propfind = true;

@@ -402,14 +407,15 @@

                 for property in container_props.iter().chain(children_props) {
                     response.set_namespace(property.namespace());
                 }

                 for item in paths {
                     if collection_container == Collection::Calendar
+                        && !mutation19_assisted_own_home
                         && item.account_id != access_token.account_id()
                         && self
                             .za_is_key_account(item.account_id)
                             .await
                             .caused_by(trc::location!())?
                     {
                         let calendar_id = if item.is_container {
@@ -1752,15 +1758,17 @@
                     if shared_account_id == access_token.account_id() {
                         continue;
                     }
                     let shared_resources = data
                         .resources(server, access_token, shared_account_id, sync_collection)
                         .await
                         .caused_by(trc::location!())?;
-                    let foreign_eligible = if sync_collection == SyncCollection::Calendar
+                    // Mutation19: omit pre-append fresh eligibility on this assisted path.
+                    let foreign_eligible = if false
+                        && sync_collection == SyncCollection::Calendar
                         && server
                             .za_is_key_account(shared_account_id)
                             .await
                             .caused_by(trc::location!())?
                     {
                         Some(
                             foreign_calendar_contexts(
```

## Mutation 48, attempt 2: killed

Required case: 10 sole delegated create (fixture-scoped; case25 separately qualified by attempt1). Source base: `e58595e529f9c9e751e9534ebc507d62a8c27a24`.
Patch SHA256: `584c656992b88becdb8d4333c3d9ef5fd989cc1653a8a0a787c318aaa971c008`. Command:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
```

Exit `101`, elapsed wall time 92.882s. Decisive observation:
`tests/src/za/sharing.rs:9575`: `case10-sole-delegated: second-only finalize removes stored SCHEDULE-FORCE-SEND`; hard parsed source check found the parameter after the delegated create.

State evidence: Whole ZA passed case25 lifecycle differential and attendee-side checks before reaching the exact case10-sole-delegated create. The input carries SCHEDULE-FORCE-SEND=REQUEST on its sole writer attendee line. The served stored source still carries that parameter when second-only finalize is omitted; the unmutated baseline passed the same absence assertion and completed strict cleanup. Runtime48.39s; total92.882s including compilation. Same frozen sharing SHA07053b2c5bfbb5c0cc5512bb71b46f349747385a9ff60c66f462c305134d8ab0.

Ruling: Valid case10 half of row48, complementing attempt1 valid case25 kill. Same physical omission at both PUT finalize calls, scoped only to UIDcase10-sole-delegated so earlier case25 does not preempt. All other HTTP cases, assertions and cleanup unchanged. Original global attempt1 remains recorded. Exact production bytes, status and index independently verified restored; no permanent production change.

| Source | Before SHA256 | Mutated SHA256 | Restored SHA256 |
|---|---|---|---|
| `crates/dav/src/calendar/update.rs` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` | `00b54c738d524867be100622c6343fc2f90f7dab4fcec714839f41cc89116be8` | `686932dedbc7f1dcdd7f5a1b688e7fe502a0fba3d851e21abb30b7a82116edd7` |

Exact temporary patch (restored from saved exact bytes):

```diff
--- a/crates/dav/src/calendar/update.rs
+++ b/crates/dav/src/calendar/update.rs
@@ -367,15 +367,18 @@
                     }
                 };
                 if first_built || second.is_some() {
                     if first_nothing
                         && let Some(second) = &second
                         && second.finalize()
                     {
-                        za_finalize(&mut new_event.data.event, second.increments());
+                        // Mutation48: the same omitted finalize, only case10's sole create UID.
+                        if new_event.data.event.uids().next() != Some("case10-sole-delegated") {
+                            za_finalize(&mut new_event.data.event, second.increments());
+                        }
                     }
                     let is_organizer = second.is_some()
                         || first.as_ref().is_some_and(|messages| {
                             messages.iter().any(|message| message.from_organizer)
                         });
                     if is_organizer {
                         if first_built {
@@ -672,15 +675,18 @@
                     }
                 };
                 if first_built || second.is_some() {
                     if first_nothing
                         && let Some(second) = &second
                         && second.finalize()
                     {
-                        za_finalize(&mut event.data.event, second.increments());
+                        // Mutation48: the same omitted finalize, only case10's sole create UID.
+                        if event.data.event.uids().next() != Some("case10-sole-delegated") {
+                            za_finalize(&mut event.data.event, second.increments());
+                        }
                     }
                     event.schedule_tag = Some(1);
                     if itip_status.is_send_sealed() {
                         let messages = if let Some(resolution) = &za_resolution {
                             za_combine_delegated(first, second, resolution)
                         } else {
                             first.into_iter().map(ZaFanOutMessage::upstream).collect()
```

## Fix round1 restored verification

The corrected test source is frozen at `07053b2c5bfbb5c0cc5512bb71b46f349747385a9ff60c66f462c305134d8ab0`. Covering wholeZA before mutation passed1/0 in392.11s, including strict final store-empty cleanup. Its first sandbox invocation compiled but failed listener/DNS setup before behavior; it is not a behavioral failure. The successful unchanged invocation used normal approved test-listener access. Both new mutant runs used the same wholeZA command, restored exact saved bytes and independently passed SHA/status/index checks. No source edit overlapped Cargo.

Fresh restored final gates, all with exit0:

| Gate | Exact command (after Rustup PATH export) | Actual result |
|---|---|---|
| WholeZA | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture` |1 passed,0 failed,357.27s, strict final empty cleanup|
| Affected units | `cargo test -p vault -p groupware -p common` |common121, groupware146, vault45 passed;0 failed|
| Workspace format | `cargo fmt --all --check` |clean|
| Working/final whitespace | `git diff --check` and `git diff HEAD --check` |clean after lossless diff-context rendering correction|

The prior shipping configuration check remains applicable because permanent production source is unchanged. Scoped independent rereview (`task-11-fix-round1-rereview.md`) approved all3 original Important findings,0 new Critical/Important/Minor. Inherited store/common unit warnings and the ZA linker warning remain explicit existing debt; no warning-free claim is made. The initial staged snapshot remains immutable, including its historical raw context-space rendering; primary staging of the final note will pick up this lossless correction. No new source code followed the reviewed freeze.
