# Zero-access JMAP calendars and browser sign-in: plan 17 outcome

Written 2026-11-17 by plan 17's Task 43, before Task 44 (the final test matrix and the whole-branch review). Plan 17 (`2026-11-14-zero-access-17-jmap-calendars.md`, its interface sheet and the six group files beside it) builds release 6 from the spec `2026-11-13-zero-access-jmap-calendars-design.md`, revision 7 (decisions D1 to D29, invariants 57 to 76). A key account's calendar works from an unmodified JMAP client in a browser: an OAuth token issued through the authorization-code flow carries a 32-byte secret after a `~`, a per-account token record (principal field 153) holds the master key wrapped under keys derived from those secrets, and the HTTP layer opens the keys on every request that presents such a token. The JMAP calendar methods read and write through the same key-account pipeline as CalDAV, which moved from `dav` into `groupware::calendar::pipeline`.

What this note can claim. Every result quoted here is local: RocksDB on one node, debug test builds, the in-store blob store unless a line says ZA-FS, a locally built thin-LTO product binary for the fresh-store script. No other store backend, no second node, no native client and no browser client was run. A passing suite is not review approval, and a review approval is not CI. The macOS linker's `__eh_frame` note appears in every suite log and is environmental. The reference client's checklist (`docs/zero-access/client-jmap.md`) is a manual gate that has not been run.

## Where things stand

- Branch `release6-jmap-calendars`, cut from the plan's branch `za-jmap-calendars-design`, on `origin/main` at `ade47a9ae` (the base after the rebase before Task 1; the checkpoints after Tasks 9, 19 and 37 and before Task 44 found the base unchanged). Head before this note: `de67af5df`, 82 commits ahead of `origin/main`. Nothing is pushed.
- Routing actually used. The plan header and the spec name Sol implementers and Astra reviewers; that routing ran Tasks 1 to 16, Task 17's first implementation and review, and Tasks 19 to 21. On the project owner's instructions of 2026-11-15, recorded in the ledger, Task 17's fix round and Task 18 had an Opus implementer and Fable reviews; from Task 22 on, implementers were Claude Opus (contract code and test modules that need judgment) or Claude Sonnet (Tasks 23, 24, 29, 34, 35 and 41, and the rows of Task 42), and the reviews of Tasks 22 to 31 ran on Claude Opus. On the owner's word of 2026-11-16, every task review and scoped re-review from Task 31's fix round on ran on Claude Fable (xhigh for identity, authorization, persistence, ordering and the oracle; high otherwise). Task 44's whole-branch review is to run on Claude Fable. The ledger's "in progress" line of each task names its implementer and reviewer. The plan header's routing lines were not edited.
- Commits, by task (`git log --oneline origin/main..HEAD`):
  - the spec and the plan: `48bb52529`, `678c052fe`, `18644d914`, `b53fa807a`, `8bae80c11`, `8cbdb1df0` (spec revisions 1 to 5), `4b245d927` (plan 17), `d22a76594` (main is the base), `f8bf55cd1` (D19, spec revision 6), `ac846e5ea` (routing), `1ba7751d7` (the preflight's alignment);
  - Task 1 `899671c8c` (the pipeline module and its neutral error);
  - Task 2 `60a7ff75a` (admission, roles and contexts move to `groupware`);
  - Task 3 `cdb612dfe` (the UID lease functions move);
  - Task 4 `fecf17ff4` (the scheduling assembly moves);
  - Task 5 `65a12d2d2` (the key create commit moves);
  - Task 6 `a704fe4a9` (one placement resolver for COPY, MOVE and JMAP);
  - Task 7 `7954a4e87` (the key update commit moves);
  - Task 8 `cd8ba6754` (plan ruling: the delete no-op receipt), `a985f16b8` (the key deletes move);
  - Task 9: no commit (the proof run at `a985f16b8`); `dba2c0c21` (plan: the fresh-store diagnostics directory);
  - Task 10 `edddb4eb3` (a pipeline create in several calendars);
  - Task 11 `fe535098f`, `0ef892222` (plan rulings), `c15d7011d` (placement and content in one update);
  - Task 12 `a538207a5` (plan ruling), `3989cf5a6` (the destroy of a whole event);
  - Task 13 `d6211346e` (token secrets, the token record, the hand-off value), fix `c963b44db`;
  - Task 14 `d75acf9b3` (field 153 and its deletion by a vault write);
  - Task 15 `f0baac1e1` (issue, open and forget);
  - Task 16 `9f7d29690` (Bearer tokens that carry keys);
  - Task 17 `88871cf02` (plan ruling D6), `01b3cd762` (the hand-off and keyed grants at the code exchange), fix `0357e3f86`; then `af3ed8934` (the approved D29 amendment incorporated into the spec and the plan, by another session);
  - Task 18 `c8f97fe69` (refresh), fix `ed32a9b2f`, `67917c44e` (documents reconciled with the fix);
  - Task 19 `4767f08e6` (the maintenance sweep of token records);
  - Task 20 `4ceabe5ba` (the calendar refusal text and the admission table);
  - Task 21 `a0ecc14df` (plan amendment), `cf698b9a7` (`CalendarEvent/get`, `Calendar/get`, `Calendar/query`); `bea178e8c` (plan amendment R21-2);
  - Task 22 `17f666208` (sync origin across pages);
  - Task 23 `1a73f868c` (`ParticipantIdentity`);
  - Task 24 `ccf25bee0` (`CalendarEvent/parse`);
  - Task 25 `e5f8fc53d` (the in-memory text index);
  - Task 26 `37de290ef` (`CalendarEvent/query` and `/queryChanges`), fix `f5eba7e79`;
  - Task 27 `dc6e3c9a8` (a shared calendar's colour in the sealed personal bundle);
  - Task 28 `beba6d2b1` (`Calendar/set`), fix `01c17f30f`, plan `a608ed6f2`;
  - Task 29 `180f76685` (default alerts by value);
  - Task 30 `81e40ef8b` (`CalendarEvent/set` create);
  - Task 31 `55c66805c` (update and scheduling), fix `ba3dbdc0f`, plan `9f52fec70`;
  - Task 32 `061d9af74` (placement);
  - Task 33 `d9da63f8e` (destroy), plan `a2e2009d4`;
  - Task 34 `0eb38ea34` (the drain from JMAP requests), fix `259fdad6d`;
  - Task 35 `799af08ce` (the calendar capability for key accounts);
  - Task 36 `aa2c77f65` (the sharing module), fix rounds `81de6e801` and `de67af5df`;
  - Task 37 `68c01d5ed` (the refusals module), fix `b53c422c5`;
  - Task 38 `b3043adf4`, `908868bd7` (plan rulings), `207b77bb1` (the differential oracle), `0baa31bf2` (plan ruling), fix `1e9be31fa`;
  - Task 39 `c3fa2099c` (plan rulings), `81162b587` (upstream's JMAP calendar tests in key mode), fix `8cf35e32c`, plan `0237a0b50`;
  - Task 40 `3d2abb4e6` (leak rules), fix `f3507acdc`;
  - Task 41 `91ff125fb` (the coverage cohort's additions and the key-mode JMAP step's description);
  - Task 42: no commit (the mutation sample applied temporary edits and reverted them);
  - Task 43 `f6f9f0dde` (this note, the reference client's checklist, `CLAUDE.md` and the release 6 documentation), corrections after its review `5cadcd739` and after its second review `7000924b4`;
  - Task 44's fix wave (`## The final review`): `3f1ce7dc5` (a fresh salt on every second-factor change), `f1c69900d` (the `code` parameter masked in traced request URLs), `f0150ec57` (a grantee's non-public events left out of a text query), `a5e45dc2f` (the smaller fixes and the missing cases); then this record of the review.
- Head of the code: `a5e45dc2f`. The commits after it change documentation only.
- **The final matrix, first pass** (Task 44, Step 2; logs in the git-ignored run directory). Rows 1 to 8 ran on `f6f9f0dde`, row 9 on `5cadcd739` and row 10 on `7000924b4`; the three differ in documentation files only, and row 10's test binary was built from the clean tree before the fix wave's first edit. Every row passed on its first attempt.

  | Row | Command | Result | Wall time |
  |---|---|---|---|
  | 1 | `cargo fmt --all --check` | no output | 7 s |
  | 2a | `python3 -I scripts/tests/ci_steps.py` | 0 differences | 0 s |
  | 2b | `python3 -I scripts/tests/test_gates.py` | 45 tests OK, 2 skipped | 5 s |
  | 3a | SHIP (`cargo check -p stalwart --no-default-features --features rocks`, with the plan header's warning filter) | clean | 151 s |
  | 3b | TM (`cargo check -p tests --tests`, with the plan header's warning filter) | clean | 61 s |
  | 4 | `cargo test -p vault -p groupware --features vault/test_mode` | vault 208 passed, groupware 59 passed | 95 s |
  | 5 | U-ws (`cargo test --workspace --exclude tests --exclude scim --exclude scim-proto --lib --bins --features "$UNIT_TEST_FEATURES"`) | 889 passed, 2 ignored, 0 failed | 222 s |
  | 6 | `cargo test -p smtp --lib --bins --features test_mode` | 16 passed | 91 s |
  | 7 | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | 7 passed | 93 s |
  | 8 | `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact` | 1 passed | 97 s |
  | 9 | ZA (`STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests`) | 1 passed (1695.50 s in the test) | 1790 s |
  | 10 | ZA-FS (the same with `BLOB_STORE=FileSystem`) | 1 passed (1607.24 s in the test) | 1753 s |

  Row 5 rewrites 26 tracked fixtures under `crates/dav-proto/resources/requests/` (the restored `parse_requests` test writes its JSON output unconditionally, with its equality assertion commented out). The same 26 files changed at Task 9; they were restored from the head while row 9 waited, and rows 6 to 8, which ran before that, do not run the dav-proto tests. The load gate for the za suites was a one-minute load below 20; on the project owner's word of 2026-11-17 it was raised to 40 for this matrix (the five-minute limit stays below 200). Row 9 had waited under 20 without starting (lowest sample 21.59) and was relaunched under 40; its highest one-minute load during the run was 37.24, row 10's 65.61, with no helper timeout. `query::text_query_cost` printed 300 events in 446 ms and 10 events in 17 ms in row 9, 405 ms and 16 ms in row 10. The pass stopped after row 10 by ruling (below): every fix of the review changes files under `crates/` or `tests/`, so rows 11 to 18 and a rerun of rows 1 to 10 run on the fixed head `a5e45dc2f` and are recorded here when finished.
- **Not run by this matrix**: the Docker suites (`directory`, `automation_tests`), the independent CalDAV client in either mode, the inventory and coverage gates, and CI on the branch (the gates' dispatch and the push need the owner's word). Evidence limits as for every result here: local RocksDB, one node, debug test builds, macOS.

## What changed and why

**Group A, the move (Tasks 1 to 9; spec 5.1, D2, D17).** The fork's key-account calendar steps left the `dav` crate for `groupware::calendar::pipeline`: admission and roles (`admission.rs`), read and write contexts (`context.rs`), the UID lease (`uid.rs`), the scheduling assembly and the status of D18 (`schedule.rs`), the create, update and delete commits (`create.rs`, `update.rs`, `delete.rs`) and the placement resolver with the event container's reseal (`placement.rs`). The functions moved with their bodies, traces, hook order, lease releases and mail-guard settlements; the CalDAV handlers kept their upstream steps and call the pipeline where their key branches stood, and `From<ZaPipelineError> for DavError` keeps CalDAV's answers. The proof is that the CalDAV tests passed without edits to their assertions: Task 9's proof run at `a985f16b8` passed global formatting, the product build (7 m 59 s, warning-free), the `groupware` and `vault` units (196 and 47), the workspace units (842 passed, 2 ignored), ZA (1495.32 s), ZA-FS (1409.10 s), SWAP (17.56 s), WK (21.12 s), WP (19.04 s), the pinned CalDAV client in key mode (its harness's 34 unit tests; 74 passed, 1 skipped, 2 xfailed) and the fresh-store script (9 of 9 readback checks after the restart).

**Group B, the pipeline paths only JMAP uses (Tasks 10 to 12; spec 5.2, 5.4, 6.5).** A create named in several calendars checks UID uniqueness under the lease in every calendar (Task 10). A JMAP update can change placement and content in one commit: `ZaPlacementPlan::jmap` resolves the old and new calendars with the rights CalDAV's `COPY` and `MOVE` ask, the event and its dead-property container are resealed in the target domain, and one physical batch carries the whole update (Task 11; `CalendarEvent::enqueue_full_at` factored out of `update_full_at` for it). The destroy of a whole event linked into several calendars runs through the shared delete commit with `za_delete_send_itip` (Task 12).

**Group C, tokens (Tasks 13 to 19; spec section 4).** The vault crate gained the token secret's codec, the derivations and associated data, the token record with its caps and reconciliation, the refresh classifier and the hand-off value, and `za_session_binding`, the 64-bit fingerprint of the credentials that D19 makes a key account's credential version (Task 13). Field 153 and its storage, with the record cleared in `vault_batch_write` exactly when the binding changes (Task 14). Issue, open and forget with the two assertions of 4.3 and the three-attempt bound (Task 15). `decode_access_token` strips a `~` suffix from every token that begins with the server's token header, the token endpoint's successful answers are untraced for every account, and the HTTP layer's Bearer arm opens keys from the record and refuses a stale generation (Task 16). The hand-off at the password step, the code exchange that issues a keyed grant, and the refresh precheck of D22 (Task 17). The refresh of 4.5 (Task 18). The maintenance sweep of D8 and the record's end with its account (Task 19).

**Group D1, JMAP reads, queries and admission (Tasks 20 to 27; spec 6.1 to 6.4, 6.6 to 6.9, D14, D15, D29).** The calendar refusal text of D25 and the admission table (Task 20). `CalendarEvent/get`, `Calendar/get` and `Calendar/query` over unsealed views, with store errors rebuilt payload-free at the boundary (Task 21, and the R21-2 amendment for every later served method). `/changes` with the sharing boundary of D15 and the explicit-origin `z` continuation of D29 (Task 22). `ParticipantIdentity`, the default switch only (Task 23). `CalendarEvent/parse` for the keyed owner (Task 24). The in-memory text index built from release 7's content part (Task 25). `CalendarEvent/query` and `/queryChanges` with text conditions matched in memory (Task 26). A shared calendar's colour in each principal's sealed bundle (Task 27).

**Group D2, JMAP writes, scheduling, sharing and refusals (Tasks 28 to 37; spec 3.1, 5.3 to 5.5, 6.5, 6.7, 6.10, 6.11, D3, D4, D10, D11, D16, D18, D28).** `Calendar/set` with its key writer as a child module of upstream's handler, a calendar destroy that sends cancellations, and `shareWith` refused (Task 28). Default alerts written by value into the sealed tree (Task 29). `CalendarEvent/set` create, update with scheduling, placement and destroy, each object its own commit, a store error answered on that object and every later one (Tasks 30 to 33). The drain runs from a keyed JMAP request that names a calendar method (Task 34). The session and principal objects advertise the calendar capability for key accounts (Task 35). The sharing and refusals modules (Tasks 36 and 37).

**Group E, closing (Tasks 38 to 44; spec 3.5, 3.6, 9.3 to 9.8).** The differential oracle runs one edit script through CalDAV and JMAP and compares the stored event's JSCalendar projection, the items and mail, the schedule statuses and the stored pairs' recipients (Task 38). Upstream's JMAP calendar tests run in key mode through `za_variants::calendars` (Task 39). Leak rules for the token record, token secrets and JMAP calendar writes (Task 40). The other harnesses, the coverage cohort's additions and the key-mode JMAP step's description (Task 41). The mutation sample (Task 42). This documentation (Task 43). The final matrix and the whole-branch review (Task 44).

## The session rule

Decided by the project owner on 2026-11-14 (spec D19; section 12, decision 4): a key account's browser sessions end only when a credential changes, that is on a password change, a recovery or a second-factor change. They survive every other vault write: the first pin's rewrap, the self-check's repair, a new recovery key, and an app password's creation, publication, withdrawal or revocation. Each such write still raises the vault revision, so resident keys are evicted and the next request opens them again from the token record; a request whose keys were opened at a generation that is no longer current answers 401 (D23), and the client's refresh then succeeds. Since the final review's fix wave, a second-factor change also draws a fresh salt (`za_totp` calls `za_set_password`), so the binding cannot return to an earlier value and the sessions it ended stay ended (`tokens::second_factor_change_ends_sessions`).

| Where | What it does | Tested by |
|---|---|---|
| `za_session_binding` (`crates/vault/src/record.rs`, Task 13) | the fingerprint of the salt, the Argon2 parameters, the verifier and the second-factor secret, never 0 | `binding_changes_only_with_credentials` |
| a key account's credential version (`crates/common/src/auth/access_token.rs`, Task 14) | is the binding | `tokens::credential_changes_end_sessions`, `tokens::other_vault_writes_keep_sessions` |
| `vault_batch_write` (`crates/common/src/auth/vault.rs`, Task 14) | clears field 153 only when the binding changes | `credential_change_clears_the_token_record`, `record_write_loses_to_vault_write`, `tokens::pin_rewrap_keeps_session` |
| `za_token_issue`, `za_token_open_access` (Task 15) | compare bindings; a lost vault assertion under an unchanged binding is retried | `tokens::issue_refuses_changed_binding`, `tokens::open_access_needs_entry_and_binding` |
| the Bearer arm (Task 16) | D23 | `tokens::bearer_crossed_by_vault_write_is_refused` |
| the code exchange (Task 17) | a crossing credential change refuses; any other crossing vault write is retried | `tokens::exchange_crossed_by_password_change`, `tokens::exchange_crossed_by_recovery`, `tokens::exchange_crossed_by_other_vault_write` |
| the refresh (Task 18) | the same | `tokens::refresh_crossed_by_credential_change`, `tokens::refresh_crossed_by_other_vault_write` |
| a browser's first invitation (Task 31, review focus 1) | the session survives the pin's rewrap | `scheduling::first_invitation_and_the_session` |
| `za_totp` (`crates/http/src/api/vault.rs`, Task 44's fix wave, FA-1) | every enrolment, replacement and removal of the second factor draws a fresh salt through `za_set_password`, so the binding cannot return to an earlier value and the tokens a second-factor change ended stay ended | `tokens::second_factor_change_ends_sessions` (kill check K1) |
| the mutation sample (Task 42) | rows J16 and J16b, both killed | |
| this documentation and the checklist row (Task 43) | state the rule | |

## Rulings

Every `Ruling:` line of the ledger (`.superpowers/sdd/2026-11-14-zero-access-17-jmap-calendars/progress.md`, git-ignored), one entry each: what was decided, why, and what it costs if wrong. Where a later ruling corrects an earlier one, the corrected text is given and the replacement is said.

### Execution and routing

- **Trailers.** The execution session's trailer set was empty at first: commits carried their prescribed subjects without copied attribution or an invented session URL. Why: no trailer identity was prescribed. Cost: none.
- **Task 44's context.** The whole-branch review gets the pinned base and head, the accepted contract and validation-only evidence, without prior findings, implementer reports or these rulings. Why: the owner's rule of an independent clean-context review. Cost: the rulings stay controller material to adjudicate the review's findings.
- **Routing from 2026-11-15** (replaces the plan header's Sol/Astra routing for the rest of the plan, without editing it). First: one Opus implementer at a time, Opus reviewers (xhigh for identity, authorization, persistence, ordering and concurrency). Then, on the owner's instruction "have the review agents run on fable", the reviews run on Fable at xhigh with implementation on Opus; later implementers Opus or Sonnet for mechanical work, task reviewers as the work's class asks, Task 44 on Fable. Why: the owner's instructions of 2026-11-15 and, for the Fable reviews from Task 31's fix round on, of 2026-11-16. Cost: a routing line to correct, no code.
- **D29's approval.** The owner approved the explicit-origin amendment and its implementation on 2026-11-15; `af3ed8934` incorporated it, and the stop before Task 22 was lifted. Why: explicit approval and a committed spec and plan. Cost: none.
- **Parallel implementer and review.** The next task's implementer may run while the previous task's read-only review runs, when their files are disjoint (first applied to Tasks 24 and 25, then 34 and 35). Why: one source writer and one cargo command at a time still hold. Cost: a fix round waits for the running implementer.

### Plan corrections

Each controller ruling that changed the plan's text has its own documentation commit:

- `1ba7751d7`: the preflight's alignment, including Task 13's use of the credential fingerprint of I §5.1 rather than `record.revision` (ruling below).
- `cd8ba6754`: Task 8's delete no-op receipt (`ids.change_id(owner, Calendar).unwrap_or_default()`).
- `dba2c0c21`: the fresh-store command's `--log-dir`.
- `fe535098f`: Task 11's single-batch placement write and its current-context hrefs; `0ef892222`: Task 11's reseal kill expectation (the grantee's readback).
- `a538207a5`: Task 12's destroy without a caller-added final commit point, with current-context hrefs.
- `88871cf02`: the D6 hand-off's assertion-only batch against its vault snapshot.
- `67917c44e`: spec 4.5 step 2 and Task 18's steps gain the superseded-generation guard of R18-1.
- `a0ecc14df`: Task 21's payload-free traces of unreadable calendars and events; `bea178e8c`: every later served key-calendar method routes keyed store errors through the payload-free rebuild (R21-2).
- `a608ed6f2`: `Calendar/set`'s alert acceptance after the commit, a head store error as the method's store error, and TenantQuota removed from the plan's rows.
- `9f52fec70`: Task 30's head states R28-2; Task 31's floor case names the R31-1 input.
- `a2e2009d4`: Task 33's attendee step follows CalDAV's decline rule (accept, then destroy).
- `b3043adf4`: the oracle compares items and the mail's calendar part by JSCalendar projection; `908868bd7`: the oracle's M6 expects pairs for the invitee and the outside address; `0baa31bf2`: the oracle's SEQUENCE rule and step M7b.
- `c3fa2099c`: three rows of Task 39's tables (F1, F2, F3); `0237a0b50`: two rows marked unchanged from upstream (R39-1, R39-2).

### Preflight

- **Task 13 uses the credential fingerprint of I §5.1, not `record.revision`.** Why: Task 13's own tests and spec revision 6's D19 require sessions to survive vault writes that change no credential. Cost if wrong: Task 13's tests fail and users are signed out after unrelated vault writes.
- **Task 17 retries a failed vault assertion when the binding is unchanged**, within Task 15's three-attempt bound; only a changed binding answers `invalid_grant`, exhausted retries and other failures answer a server error with the grant cleaned up. Why: spec 4.3 and 4.4 and Task 17's own race test. Cost if wrong: a successful race with an unrelated vault write would fail.
- **Every submitted `blobId` link is refused**, including a same-event embedded blob; `za_take_rulings` loses its unused stored argument. Why: spec 6.5 and 6.11 refuse attachments on every patch. Cost if wrong: unsupported attachment behaviour.
- **I §7.3's mapper documentation follows D28** (per-object handling). Why: spec 5.3 and I §7.5 keep earlier committed answers. Cost if wrong: a stale comment that could hide committed objects.
- **`ZaDeleted` and CalDAV's deletion behaviour stay as they are**; no new post-commit receipt field or failure hook. Why: Task 8 produces only `AccessToken` invalidations, and `invalidate_caches` propagates errors only from role expansion, so the reported post-commit error is unreachable today. Cost if wrong: a future fallible post-commit step needs committed-receipt handling first; a pre-commit hook is not post-commit coverage.

### Tasks 1 to 12

- **Task 1.** The `pipeline` module declaration follows `pinning` in rustfmt order. Why: FMT and the interface contract. Cost: none.
- **Task 8.** The CalDAV delete's assertion-only no-op keeps `ids.change_id(owner, Calendar).unwrap_or_default()` instead of `last_change_id`. Why: a concurrent unlink can leave the archive under another parent, the storage step then writes nothing and the base answers 204, while `last_change_id` debug-panics on the absent id. Cost if wrong: none found; the receipt is 0 only when no Calendar change was made.
- **Task 9.** FRESH gets the required `--log-dir` (a new directory). Why: the script requires it and CI supplies it. Cost: none; only the diagnostics destination changes.
- **Task 9, generated fixtures.** `dav-proto`'s restored test regenerated 26 tracked JSON files during U-ws; they were restored from the commit with guards, and the report distinguishes a parse success from an absent snapshot comparison. Cost: none to the product.
- **Task 11, revert.** The reseal fault is reverted from an exact saved pre-fault file, not by `git checkout`, so the uncommitted placement implementation survives. Cost: none.
- **Task 11, atomicity.** A narrow factoring in `storage.rs`: `CalendarEvent::enqueue_full_at` has `update_full_at`'s body without its commit point; the placement path calls it on both attempts with no trailing commit point, so one physical batch carries the update. Why: the embedded commit point could split the event and container from later alarms and tombstones. Cost: none for existing callers, which keep the wrapper.
- **Task 11, hrefs.** Vanished hrefs on every attempt use the asserted parent context's current canonical names. Why: a source rename during a pause would otherwise log an obsolete href that incremental sync filters out. Cost: none to CalDAV paths.
- **Task 11, the kill oracle.** The missing-reseal kill is shown by any post-move dead-property readback, with the grantee's decisive: the shared owner keeps its key and can still open the unresealed container. Cost: none; the test order stays.
- **Task 12.** The whole-event destroy omits its caller-added final commit point on both attempts and uses the current-context href rule. Why: a final split could publish the deletion and then fail a context-only write, losing its receipt and mail. Cost: none to existing CalDAV paths.

### Tasks 13 to 19

- **Task 13 (M1).** A strict 43-byte alphabet and final-sextet check runs before `decode_slice`. Why: the decoder's malformed-tail diagnostics re-decode into unzeroed scratch. Cost: none; accepted inputs are identical.
- **Task 14, existing OAuth assertions.** No assertion that a non-credential vault write ends an OAuth session needed a changed outcome; `setup.rs` and `password.rs` joined the task's scope for wording and for an exact `za_session_binding` comparison. Cost: two test files.
- **Task 14, a temporary `#[allow(dead_code)]`** on `token_write_store` until its first caller in Task 15, which removed it. Cost: none.
- **Task 16, Bearer lookup errors** map to a fixed `AuthEvent::Failed` and 401 with no payload. Why: I §5.5 and spec 8. Cost: none to Basic.
- **Task 16, D27's evidence.** The fifteen-refusal control stays, with a recorded limit: it does not prove zero counter increments, since the counter API is private. Cost: D27's zero-increment claim rests on reading the new paths for an absence of the ban counters.
- **Task 17, D6.** The password step's hand-off writes one more assertion-only batch against the original vault snapshot before it returns the secret; a lost assertion is an authentication failure without retry, and a failed operation deletes the provisional entry. Why: a vault write between the password's check and the hand-off's write must refuse the step. Cost: one batch per keyed hand-off.
- **Task 17, cleanup scope.** The cleanup rule applies to the new keyed issuance; upstream's failures after `create_oauth_grant` and before the keyed issue may leave a grant row, as upstream does. Cost: an inherited grant row with no wrap.
- **Task 17, the R17-1 regression** uses inert synthetic canary bytes at a synthetic code's hand-off alias; no real key, wrap, code or token is a canary. Cost: it does not reproduce a real hand-off's disclosure.
- **Task 18, R18-1.** In `za_token_refresh`, after the attempt's registry read and before classifying, a stored generation of the presenting grant above `presented` answers `Superseded`: nothing written, nothing deleted. Why: this request's rotation left the stored generation at `presented` and it never falls, so a higher one is a later rotation. Cost if wrong: a retry is refused where it could have been answered; the client already holds the newer token. The spec and plan text were reconciled in `67917c44e`, replacing the earlier note that they were owed.
- **Task 18, R18-6 and R18-8**: the code stands (no fork trace for `Superseded` and `VaultChanged`; an unusable record makes a refresh delete its grant while the sweep leaves the record). Both are questions for the spec (below).
- **Task 18 documents.** Task 18's documents were reconciled with the accepted fix before Task 19, and the overstated refresh vault-assertion catcher claim removed. Cost: documentation only.

### Tasks 20 to 27

- **Task 20, RED.** The tests stay in the compiled source; a missing-function diagnostic is the accepted RED. Why: an absent test file proves nothing. Cost: one more compile run.
- **Task 21, traces.** Recoverable unreadable-calendar and unreadable-event branches trace fresh `DataCorruption` events with account and document ids and fixed reasons, never the original error context. Why: the deserialization error can carry raw archive bytes. Cost: less detailed corruption diagnostics.
- **R21-2.** Every key-calendar method a later task serves routes its keyed store errors through the same rebuild: dispatcher-admitted methods through `za_serve_calendar_read`, the three admitted inside the changes handlers through the same rebuild applied there. Why: spec 8 binds every path, and `CalendarEvent/query` reads content through the reader whose exposure R21-1 fixed. Cost: one wrapper per arm and two regressions.
- **Task 26's `/queryChanges` call** in the payload-free case. Why: the handler form has no other regression. Cost: one call.
- **D28's trace.** The per-object store-failure trace, and the trace of a store error in `Calendar/set`'s default write, are fresh events with the account id, the document id when known and a fixed reason. Why: the original may carry stored bytes. Cost: less detail.
- **D15 is a sync-correctness rule, not an access control.** A client-chosen origin can only make the server answer from log records for a range, which hold item ids, never content. Cost if wrong: a grantee that forges a state could learn ids changed or destroyed before its grant or rename. Listed for the owner to confirm.
- **Task 23's parity** with ordinary accounts is shown by one JP run in Task 24's closing checks. Cost: one plain JMAP run.
- **Task 24's JP flake.** JP's first attempt failed in upstream's `mail::thread_merge` (thread sizes, ordinary accounts) and the rerun on identical source passed; accepted as timing outside the task, since thread merging finishes in a background task. Cost if wrong: an ordinary-account threading regression masked; both runs are kept. The ledger records no recurrence in later JP runs.
- **Task 26's kill check** of the `/queryChanges` handler form closes its evidence gap: the case fails when the rebuild is missing, and Task 21's R21-1 RED showed on the same reader that an unrebuilt error carries the stored bytes. Cost if wrong: the canary assertion itself stays unobserved failing on that path.

### Tasks 28 to 33

- **TenantQuota.** The enterprise-gated arm and its gated assertion are removed; a tenant-quota error, which exists only in enterprise builds, takes the D28 store-failure path. Why: code under the enterprise feature is Stalwart Enterprise Licence code. Cost: an enterprise build answers a tenant-quota failure with the generic store-failure error.
- **R28-1.** `request_alerts.accept` moves after `commit_batch` returns `Ok`. Why: with per-object commits a failed object must not leave its alert ids in the request state. Cost: none in stored state.
- **R28-2.** A store error from `za_writer_keys` is the method's store error, rebuilt at the dispatcher, not the keyless refusal; the same rule binds Tasks 30 to 33. Cost: none.
- **D28's trace evidence.** `store_failure_stops_the_call` captures traces and asserts the fixed reason and that the hook's own details text appears in no event. Cost: one assertion block.
- **R31-1.** `update_applies_sequence_floor` passed with the floor removed, because upstream's increment stores the stored SEQUENCE plus one for an owner-organized event. Its input became a grantee-organized event (CalDAV `SEQUENCE:5`, JMAP `sequence: 1`), which upstream's increment skips and the floor raises; the stored value must equal the floor; a kill check followed. Cost: one changed case and one kill run.
- **R31-6.** One update assertion refuses `hideAttendees: true` with the stored event unchanged. Why: Task 26's query relies on no key event carrying the flag. Cost: one assertion.
- **Task 33's attendee case follows the code.** A destroy of an unanswered invitation sends no DECLINED, as CalDAV's DELETE (upstream skips a NEEDS-ACTION attendee); the case accepts first, then destroys. Why: D18 gives a destroy CalDAV's condition. Cost if wrong: a decline on an unanswered invitation would be a product change against CalDAV parity.

### Tasks 34 to 37

- **Task 35 beside Task 34's review** (disjoint files). Cost: a Task 34 fix round waits.
- **R34-2.** The drain hook drains the caller's own inbox whatever account the methods name: one step with an item pending for key2 and a method naming key's account. Cost: one step.
- **Task 36's additions.** (a) the view grantee's destroy asserts upstream's text "You are not allowed to remove events from calendar {id}."; (b) a seventh case, `move_with_scheduling_into_shared`, a placement change with a fan-out (R32-2). Cost: one case.
- **Task 36's deviations** stand as test-side corrections: trigger checks match the value whatever its parameters (upstream writes `TRIGGER;RELATED=START`), and the delegated-reply fixture is a mailed REQUEST the owner's drain imports and the owner moves. Cost if wrong: a fixture that hides a defect in a planted or PUT attendee copy.
- **R36-10.** Upstream's JMAP create falls back to `itip_attendee_create` when the account is not the organizer; the key writer calls `itip_create` only, so an attendee that creates an already-answered event with `sendSchedulingMessages: true` sends no REPLY and gets no schedule tag. Accepted under D1 (CalDAV has no attendee create); a proposed addition to spec 3.3 and an open item for the owner. Cost if wrong: one missing REPLY path in a browser client.
- **Task 36, fix round 1.** A new case `grantee_destroy_sends_delegated_cancel` (a grantee's JMAP destroy and its CalDAV twin each send one CANCEL with the grantee as `SENT-BY`), and the stored pair read before and after the scheduled move with a kill check. Why: the dispatch as made did not show what R32-2 asked. Cost: one round.
- **Task 37's addition R35-3.** The keyless Bearer token's session object lists the key account with the calendar capability (D13). Cost: three assertions. The same dispatch resolved the D29 case's wording: real `z` states of both origins from the key account's own paged `/changes`; `stateMismatch` with a read-back on `Calendar/set` and `CalendarEvent/set`; a `z` in `ifInState` never changes the keyless or the non-grantee answer, and a difference is reported, not pinned.
- **Task 37's deviations** stand: `notification_destroy_leaves_the_item` runs before `every_row_of_6_11`, the keyless Bearer token is minted with `za_grant_token`, the metadata value is built inline, `CalendarEvent/copy` carries `create {}`, and one temporary upload is left (the final emptiness check passed). Cost: small.
- **Task 37, fix round 1.** R37-1 (a matching `ifInState` is accepted), R37-2 (a third impersonation form: a key account holding `Impersonate` with a view grant and its own password), R37-4 (the attachment download is denied), R37-5 (metadata refused on update). Why: each is part of spec 9.2's list for the module. Cost: one round.

### Task 38, the oracle

- **The dispatch.** The kill check runs on the committed tree, the kill mutation is confined to `ZaEventWriter::destroy`, and an oracle difference stops the implementer rather than being normalised away (spec 3.6).
- **Items by projection.** Items and the mail's calendar part are compared as `normalize_jscal(jscalendar_of(tree))`; other mail parts with `Tally::same`. Why: spec 3.5 compares by recipient, method, UID, SEQUENCE and recurrence id, and by D1 a JMAP write stores the converter's form (`DURATION` for `DTEND`, the export's order) with the stamps D17 assigns to JMAP's shaping. Cost if wrong: a property the converter drops would pass; statuses and pairs are compared separately.
- **Deviations** stand: participant ids derived from mapped names are mapped too, an object-valued `calendarIds` is dropped whole, the script's open parts are the implementer's, item trees are opened before drains, and Part B's refusal is arranged by writing the pin record.
- **The SEQUENCE rule.** The script's texts carry an explicit SEQUENCE, the same on every VEVENT: 0 at the create, one higher at each organizer step that changes the text. Why: the increment is JMAP's shaping for a client that omits `sequence`; a script whose CalDAV text never raises SEQUENCE differs by itself. The rule's original "cost if wrong" said Task 31's floor case and the events module cover the server's increment for a client that omits `sequence`; that was corrected: they do not (R38-1), which led to step M7b.
- **M6's pairs** name the invitee and the outside address: `token_union` mints a pair for every scheduled attendee not local to the sender (release 5 spec 5.2). Cost if wrong: none found.
- **R38-1, step M7b**: between M7 and M8 the master ends one hour later with scheduling, the JMAP patch without `sequence`, so upstream's increment and the pipeline face the CalDAV client's bump; R38-2: a message's calendar part has its own METHOD line compared as text. Cost: one round.

### Tasks 39 to 42

- **F1, a D11 row.** In a windowed or property-subset read, each override whose title differed from the master's when the defaults were written holds the patch `alerts/abc/iCalendar` with that title. Why: D11 writes the alarms per component as upstream's iCalendar view does, and a read is upstream's projection; an ordinary-account control (alarms with different descriptions over CalDAV) showed the same patch. Cost if wrong: a client sees an unknown patch path, and a CalDAV client shows the title the event had when the defaults were written. Open for the owner: a constant description would remove both effects and needs a change to D11's wording. What a client sees is inherited from upstream's two read paths, for every account: a windowed or property-subset read shows the patch, a full read does not.
- **F2.** The share endpoints write the calendar's ACL through upstream's builder, which logs one share notification per entry change with ids and rights only; the variant mirrors upstream's lines with the level's rights. The plan's "the share endpoints write none" was a wrong premise.
- **F3, a 6.7 row.** A view grantee's empty `Calendar/set` patch is `forbidden`: the key writer takes the context with `Acl::Modify` before computing a change. Cost if wrong: an empty update to a view-level shared calendar errors.
- **Task 39, fix round 1.** R39-1: upstream's two assertions on Jane's event update and destroy at view hold unchanged and are restored (upstream's own check answers first in upstream's words); R39-2: Jane's calendar name is compared as upstream does; R39-6: `calendar_notification` ends with `test.assert_is_empty()`; R39-4: two availability assertions changed, not four. Cost: one round.
- **Task 40's deviations** stand, subject to review (the base64 narrowing in particular, later replaced by R40-2).
- **Task 40, fix round 1.** R40-1: the authorization code is taken with the label-only form, since `unwrap_code()`'s panic would print a consent token with its hand-off secret; with it R40-2 (slicing by character, no alignment left unsearched), R40-3 (scans wait for the workers), R40-4 (one scan of the owner before the task's destroy), R40-5 (the refresh entry's next wrap length), R40-6 (each canary and both colours read back through a keyed get) and R40-10 (canaries also searched in base64 and hex). Cost: one round.
- **Task 41, step 1.** The script's seven baseline names are functions earlier releases retired, each with a disposition; nothing is restored and the inventory gate stays the authority.
- **Task 42, batches.** Sequential batches of about seven rows, Sonnet executors, one row per background command, each batch writing its table as it goes; base `f3507acdc`. Why: an executor holding 28 rows hands back early or loses rows.
- **The ZA-short recipe** keeps the four `setup::` calls (`tests/src/za/mod.rs:142-145` at that base), which provision the suite's key accounts, and removes `disabled::test` through `sharing_races::test`; J4 was run again with it as J4s.
- **The controller's rows X1 to X3** (later X4), defined in batch D's dispatch.
- **X1** is a real gap in the fork's own code: a quiet move that lost the stored pairs would leave the organizer's bundle without them. The sharing module's move case was strengthened (Task 36's fix round 2) and X1 run again.
- **X2, as corrected.** The first ruling said the call of `increment_sequence` and its place before the floor are killed by X3 and J6; the review (R42-1) showed the logs do not support it, and the ruling was replaced: X2 stays an accepted residual, not an equivalent mutant, because the mutated function is upstream's, byte-identical to v1.0.3, which the key writer calls by D17. No za case was shown to compare an unscheduled JMAP update that omits `sequence`, and upstream's unit tests of `sequence.rs` are in the private tree no build here compiles, so nothing in this fork's net covers the override branch of the increment for any account. Row X4 (the call removed from the key writer) was added to replace the claim with evidence; its result is under the mutation runs.
- **The X2 and X4 record** (replaces the sentence R42-1 corrected). The key writer's call of upstream's increment is protected by the oracle's Part A (X4's kill), and a wrong extra raise by the oracle's M2 (X3's kill). M7b compares the scheduled path end to end and has killed no mutant of the increment, because the scheduling path repairs it (X2, X4). The override branch inside upstream's `increment_sequence` on an unscheduled update stays an accepted residual in upstream's function (X2).
- **Task 42's close.** All rows run: the brief's 28 killed; X1 killed after the test was strengthened (`de67af5df`), X3 and X4 killed, X2 an accepted residual. Task 42 made no commit; the strengthening is Task 36's.

### Tasks 43 and 44

- **README's search sentence** (Task 43's concern 4). The README's "calendar full-text search stays gated" is corrected in Task 43's fix round, though the brief's README row covered only the deferred list: D12 serves text search of a key account's events over JMAP, so the sentence told a reader of the public README something release 6 made untrue. The fix states what is served and what stays as it was for CalDAV, read from the code and the spec. Cost if wrong: one sentence of the README.
- **Task 44's matrix beside Task 43's review.** The matrix starts on `f6f9f0dde` before Task 43 is closed: a fix to Task 43 can only be a documentation commit, which changes no file a matrix row compiles or runs. Each row records its head and the tree hashes of the source directories and the Cargo files; the product build, its verifier and the fresh-store script are held until no documentation commit is pending, since the verifier compares the binary's source commit with the head. The plan itself puts a documentation commit after the matrix. Cost if wrong: a row rerun on the later head.
- **R43-3.** The project rule governs: the outcome note names an inherited behaviour without describing how it arises, and the paged-`/changes` item is not an accepted residual, since nobody accepted it; it moves to the open items as pending the owner's decision. Why: the note is public. Cost if wrong: the public note says less than it could.
- **R43-4.** Release 4's sharing spec and plan 14's outcome note are not edited by Task 43: they are approved records of their own release, and a pointer in them is the owner's call. Their four statements of the colour exclusion D14 replaces are recorded under known staleness and put to the owner, with the observation that spec section 10's amendments do not list the sharing spec's colour statement. This supersedes the earlier ledger line that asked for a note in those documents. Cost if wrong: one more small edit when the owner asks for the pointers.
- **R43-6 and the twelve minors** (R43-7 to R43-18) are corrected in the same fix round, though the brief's tables did not list those rows: each is a statement of fact in a public document or in the file every later session reads first, and leaving them would have the whole-branch review spend its one pass re-finding them. Cost if wrong: a larger fix diff for the scoped re-review.
- **Five lenses for the whole-branch review.** Task 44's whole-branch review runs as five independent reviewers in one pass, each with a lens (A sign-in, the token record and the session rule; B the pipeline, the move out of `dav` and parity with CalDAV; C1 `CalendarEvent/set`, scheduling and default alerts; C2 admission, reads, `Calendar/set`, queries, paged sync and sharing over JMAP; D leaks across the change, the tests that changed, the gates and the documents), not one reviewer. Why: the branch is 118 files and about 42,800 added lines, more than one reader can read closely in one context, and the project's earlier whole-branch reviews ran as lenses. This is the one whole-branch pass the owner authorized. Cost if wrong: an issue between two lenses' slices is seen by neither; each lens was told to follow calls out of its slice.
- **Independent briefs.** The initial briefs carry no earlier finding, implementer report, task-review conclusion or ruling of this note, and the reviewers do not open this note or the run directory; the ledger's 222 deferred lines and 40 coverage notes are put to each lens only after its report, for triage. Cost if wrong: a second, shorter turn per lens.
- **The review before the matrix finished.** The review was dispatched on `5cadcd739` while the matrix's suite rows still waited on the load gate, with an extract of the rows already run. Why: the gate's wait was outside anyone's control, the review reads only, and a matrix failure is adjudicated whatever the reviewers knew. Cost if wrong: a reviewer spends attention on something a failing row would have pointed at.
- **N1 (Task 43, second round).** The note's R43-3 entry now says that it names an inherited behaviour without describing how it arises, and that the item is pending the owner's decision. The wording fault was the controller's: the earlier text was copied from a private record into this public note. Cost: none beyond the edit.
- **N2 to N8 (Task 43, second round)** are corrected in the same round, each one phrase; this note's "Rulings" was not extended then to the rulings after `5cadcd739`, which this record adds. Cost: a second small fix diff to re-review.
- **The matrix pauses after row 10.** Rows 11 to 18, with a rerun of rows 1 to 10, run once on the head the fix wave leaves. Why: every fix changes files under `crates/` or `tests/`, so most rows would be rerun anyway, and one complete matrix on the final source is stronger evidence than rows split across two heads; the full-profile product build is not spent on a head about to change. Cost if wrong: a fault in rows 11 to 18 that predates the fixes is found one step later and cannot be separated from them by the record.
- **FA-1 = FD-2** is fixed by drawing a fresh salt on every second-factor change, in the TOTP endpoint, through the existing `za_set_password`: the binding then never recurs, the change is atomic with the vault write, and no stored layout, registry object, D19 input or D19 text changes. Deleting the grants on a binding change was weighed and not taken (not atomic; a failed deletion leaves the revival in place); a guard in the commit funnel that refuses a binding change under an unchanged salt was left out of the wave and offered to the owner. Cost if wrong: one more Argon2 derivation per second-factor change, and a future second-factor endpoint that does not hold the password could not use this fix.
- **FA-3 = FD-1** is fixed by masking the value of every `code` query parameter where a request URL is formatted for the trace, and in a traced `Referer`, for every request: no account is known when the URL is traced. A third every-account trace exception beside D24 and D26, it needs the owner's confirmation as an amendment of spec section 10; until then it is the controller's ruling. Cost if wrong: an operator no longer sees an authorization code in a debug URL trace.
- **FC2-1** is fixed in the query's text resolution: for a caller that is not a member of the account, an event whose unsealed tree is not public enters no text index, as on an ordinary account (D12) and as `/get` shows it (6.3). CalDAV still serves such an event whole to a grantee (FC2-2), field policy 2's recorded limit, which goes to the owner as a line for spec 3.3. Cost if wrong: JMAP is stricter than CalDAV in one more place for such an event.
- **FC1-1 = R36-10** changes no product code: the code is what D1 asks for, and a fallback could only sit in the pipeline's create arm, which CalDAV shares and releases 2 and 5 qualified. The wave adds one case that pins the behaviour; the owner is asked to add the line to spec 3.3 or to amend 3.2. Cost if wrong: an invitation accepted by creating the copy goes unanswered until the owner rules.
- **FA-2 and FC1-2** are fixed as tests, FA-2 with a kill check that shows the case can now fail. Cost: the runs.
- **The Minor findings** are disposed by id as the adjudication lists (the table under "The final review"): the wave takes a broken property, a missing test of a spec requirement or a one-line correction, and leaves what changes a client-visible answer under a fault, what the plan prescribed, and what only the owner can decide. Cost if wrong: a deferred or residual item is fixed in a follow-up, with its record here.
- **The gate of 40 for the remaining runs.** The fix wave's runs and the second matrix pass use the gate the owner set for this matrix: the same task's runs on the same host in the same hours. Cost if wrong: a load-induced helper timeout, which the rerun rule answers.
- **P4 gains `za_refresh_event`'s empty-list refusal** (`Forbidden(Context)`), the same class of fault as FB-1 in a function the wave otherwise leaves as moved; covered by reading only, since its trigger needs a lost assertion and a reclassified account in between. Cost if wrong: a refusal where a panic was.
- **W-1 to W-5 are parked, with no second fix wave**: W-1 deferred with its fix named, W-2 a residual (an equivalent mutant at suite level), W-3 and W-4 deferred test gaps, W-5 a residual stated with the masking's rule. The second matrix pass continues on `a5e45dc2f`, since no code changes after the re-review. Cost if wrong: each is a small follow-up; none stores, serves or traces more than before the wave.

The load gate's change from 20 to 40 for this matrix was the project owner's own instruction of 2026-11-17 ("let's raise it to 40"); it is recorded here as his decision, not as a ruling.

## Review focus

1. **The first invitation keeps the browser signed in** (Task 31, `scheduling::first_invitation_and_the_session`). Passes: after the invitation to another key account pinned the invitee's key, the same token reads the event, its refresh is granted, the grant is still in the token record, the vault revision rose by one and the binding is unchanged. Step 4 checks the CalDAV body's `SCHEDULE-STATUS` of `1.2`, because upstream's JSCalendar projection drops `SCHEDULE-STATUS`. Limit (R31-8): the case shows that a vault write happened, not that it was the pin's rewrap.
2. **A native client's dead properties across a browser move** (Task 11, `pipeline::placement_reseals_event_container`). Passes; with the reseal removed it failed at the grantee's readback (a per-item 500 with no dead-property values), while the owner's readback still succeeded because a shared owner keeps the key that opens the unresealed container.
3. **A grantee's default reminders** (Task 36, `sharing::grantee_default_alerts`). Passes: the grantee's first event with `useDefaultAlerts: true` stores one alarm with the owner's `-PT10M` (the grantee's entry is a copy of the owner's from the grant); after the grantee sets its own email alert `-PT45M`, its second event stores `-PT45M` only, with `ATTENDEE:mailto:jmapcal-sharing-reader@example.com` (the grantee), reads back `useDefaultAlerts: false`, and holds no event preferences. Its kill check (the writer's account id in place of the grantee's personal id) failed at the second event's alarms. Limit (R36-5): the first event cannot tell the grantee's copy from the owner's live entry.
4. **An edit in the browser drops what a native client wrote** (Task 38, `oracle::round_trip_keeps_what_an_ordinary_account_keeps`). Passes: on the key account `X-JMAPCAL-KEEP`, `DESCRIPTION:alarm text` and `X-JMAPCAL-ALARM` survived the JMAP edit, and its lines equal the ordinary account's. Part B: after an unscheduled and a scheduled JMAP update, the pairs' recipients, the display name and the withheld entries are what they were. Row X4 was killed here (below).
5. **Search on a large calendar** (Task 26, `query::text_query_cost`). The latest passing suite run (Task 36's fix round 2) printed `text_query_cost: 300 events opened, one text query without a time range, 440 ms` and `text_query_cost: 10 events opened, the same query within one day, 17 ms`; Task 26's own run printed 687 ms and 16 ms. Both are a debug build on local RocksDB, inside the za suite. The counts are by construction: the case asserts the account holds exactly the 300 events before the unranged query, and no counter verifies the number of views opened. A networked store would add a read latency per candidate (R26-6), and memory was not measured.

## Mutation runs (plan 17)

### Task 42, the brief's 28 rows (spec 9.7)

Each row was one exact replacement applied to `f3507acdc`, run as its row names (ZA: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`, which stops at its first panic; U-vault, U-groupware, U-common: the named unit test), then reverted with `git checkout -- crates/ tests/`; each batch ended with an empty `git status --short crates/ tests/`. Logs and diffs are in the git-ignored `mutations/` directory. The task's review read every diff and log and confirmed each first failure. **28 of 28 rows were killed.** The first failure was at the named catcher, or one the row allows, for 23; at another test of the same property for 5 (J3, J7, J16b's suite run, J19, J25). File and line are as of `f3507acdc`.

| Id | Fault | Edit | First failure observed | Named catcher? |
|---|---|---|---|---|
| J1 | the JMAP create skips the lease (its UID check) | `calendar_event/set/za.rs`, create: `za_take_uid_lease` gets `&[]` | `tests/src/utils/jmap.rs:586`, "Missing not created item 0": the third create of `jmapcal-events-uid` was created | yes, by inference: `events::create_uid_rule` |
| J1b | the lease check covers one calendar | `pipeline/uid.rs`, `za_take_uid_lease`: `take(1)` | `pipeline.rs:662`, "create_uid_taken_in_second_calendar [a, b]: inserted conflicting document 4" | yes |
| J2 | the JMAP create skips sealing | `calendar_event/set.rs:189`: the key branch never taken | `utils/jmap.rs:574`, "Missing created item 0", `notCreated` `forbidden` "Another process modified this calendar event, please try again." (the ordinary fence) | yes, by inference: `events::create_reads_back_over_caldav` |
| J2f | the same, without the fence | J2 and the fence line removed | `events.rs:976`, "the stored tree holds a sealed value in the clear" | an earlier `events` case the row allows (`create_reads_back_over_caldav`) |
| J3 | the JMAP create calls `ItipMessages::queue` | `set/za.rs`, create: upstream's builder queues the tree's messages (corrected once: the first edit had no document set) | `scheduling.rs:475`, "invite_key_outside_and_plain: mail to OUT", left 2, right 1 | no: an earlier scheduling case (the queued task's mail doubled); `no_itip_task_ever` not reached (R42-3) |
| J4 | an update drops the token pairs | `pipeline/update.rs`, `za_commit_update`: `pairs = None` | `tests/src/za/fanout.rs:423`, "the token is stable across updates" (CalDAV; the function is shared) | a module before `tokens::test`, as the row expected; follow-up J4s below |
| J5 | an update drops the display name | `set/za.rs:1390`: `display_name: None` | `utils/webdav.rs:1008` from `events.rs:1455`, left `{""}`, right `{"Display canary"}` | yes: `events::update_keeps_pairs_and_display_name` |
| J6 | an update skips the SEQUENCE floor | `set/za.rs:1188`: the `za_sequence_floor` call removed | `events.rs:1514`, "update_applies_sequence_floor: the stored SEQUENCE is not the floor", left 1, right 5 | yes |
| J7 | a text filter reads the stored tree | `calendar_event/query.rs:549`, expanded loop, key arm yields the stored content | `query.rs:518`, the expanded `title: offsite` query, left 0, right 1 | the catcher is `query::two_accounts_same_answers`' expanded assertions (`query.rs:512-525`), the only place the suite checks the expanded filter against the view; the brief's `text_matches_the_view_not_the_tree` runs unexpanded queries and cannot catch it (R42-2) |
| J8 | the unexpanded matcher reads one component at a time | `za_text.rs`, `ZaEventTextIndex::insert`: an inline copy of `index_document` with `take(1)` (corrected once for an import) | unit `za_text.rs:315`, "Title \"team offsite\" English"; ZA `query.rs:435`, "condition 8: {\"title\":\"team offsite\"}", left `[]`, right `["jmapcal-query-series"]` | yes, both (`text_index_cases`; `two_accounts_same_answers`) |
| J9 | a token-record write omits the vault assertion | `common/src/za/tokens.rs`, `token_write_store`: the vault assertion removed | unit `common/src/za/tokens.rs:786`, `unwrap_err()` on `Ok`; ZA `za/tokens.rs:1024` in `assert_invalid_grant` | yes, both (ZA by inference: `exchange_crossed_by_password_change`) |
| J10 | a superseded refresh generation deletes the grant | `http/src/auth/oauth/za_token.rs`: `Superseded` deletes as `Lost` | `za/tokens.rs:2218`, "the superseded retry deleted the grant" | yes: `retry_of_g_while_g1_completes` (iteration not logged, R42-5) |
| J11 | a lost record race puts back a pruned entry | `tokens.rs`, `za_token_refresh`: the retry writes the first attempt's record | `za/tokens.rs:1024`, the retry came out granted | the named case, one assertion before "generation g is in the record" (R42-5) |
| J12 | the sweep keeps a refresh entry below the stored generation | `za_token_sweep`: generation 0 given to `reconcile` | `za/tokens.rs:2430`, "D8: only the stored generation survives", left `[0, 1]`, right `[1]` | yes: `interrupted_refresh_cleared_by_sweep` |
| J13 | the drain hook is removed | `api/request.rs:207`: the `za_drain` call removed | `scheduling.rs:1380`, "drain_runs_from_jmap: the item is still pending after Calendar/get" | yes |
| J14 | a notification destroy is let through | `api/za_calendar.rs:168`: `(CalendarEventNotification, Set)` served | `refusals.rs:933`, left `None`, right `Some("accountNotSupportedByMethod")`; the destroy answered `notFound` and the item stayed | yes: `notification_destroy_leaves_the_item`; the kill is the gate's answer, not the item-survival assertion (R42-6) |
| J15 | the wrap's associated data omits the grant id | `vault/src/keys.rs`, `token_aad` | unit `vault/src/tokens.rs:445`, `other_grant.open_access(..).is_none()` | yes (unit only, as the row says) |
| J16 | a credential change leaves the token record | `auth/vault.rs`, `vault_batch_write`: the clear removed | unit `auth/vault.rs:1568`, `raw_token_record(..).is_none()`; ZA `za/tokens.rs:937`, "unexpected token record after password change" | yes, both (ZA by inference: `credential_changes_end_sessions`) |
| J16b | a vault write that changes no credential deletes the record (D19) | the clear made unconditional | unit `auth/vault.rs:1593`; ZA `za/tokens.rs:937`, "unexpected token record after first pin" | unit yes; ZA: `pin_rewrap_keeps_session`, an earlier case of the same property; `other_vault_writes_keep_sessions` not reached (R42-4) |
| J17 | a superseded refresh entry is kept | `vault/src/tokens.rs:188`, `reconcile` | unit `vault/src/tokens.rs:665`, left `[0, 1]`, right `[1]`; ZA `za/tokens.rs:2340`, same values | yes, both |
| J18 | an unopenable secret is served keyless | `za_bearer.rs:54`: the catch-all arm returns `Ok(None)` | `za/tokens.rs:341`, "session case 0", left 200, right 401 | yes: `tampered_or_foreign_secret_is_401` |
| J19 | the hand-off is not deleted on redemption | `za_handoff_take`: no delete | `za/tokens.rs:1195`, the consent hand-off still stored after redemption | no: `consent_path`, an earlier case of the same property (R42-4) |
| J20 | the cap's eviction leaves the evicted grant | `za_token_issue`: the eviction's deletion skipped | `za/tokens.rs:747`, the evicted grant still stored | yes: `cap_evicts_least_recently_used` |
| J21 | a refresh without the secret of a keyed grant rotates it | `za_token_refresh_precheck`: the guard made always true | `za/tokens.rs:1024`, not `invalid_grant` | yes: `refresh_without_secret_of_keyed_grant_refused` |
| J22 | a Bearer request at a stale generation is served | `authenticate.rs:285`: the stale-generation refusal removed | `za/tokens.rs:446`, left 200, right 401 | yes: `bearer_crossed_by_vault_write_is_refused` |
| J23 | a successful token answer is traced | `oauth/token.rs:738`: `with_untraced_body()` removed | `za/tokens.rs:1892`, "the trace contains an OAuth token or a secret" | yes: `granted_answer_is_untraced` |
| J25 | the unexpanded matcher's document is built from the stored tree | `query/za.rs:93`: the stored content inserted | `query.rs:435`, "condition 1: {\"title\":\"budget\"}", left `[]`, right `["jmapcal-query-meeting"]` | `two_accounts_same_answers`, the same property; the named `text_matches_the_view_not_the_tree` runs later and was not reached, and would catch it by reading (R42-4) |
| J26 | a placement change asks for a right `MOVE` does not | `placement.rs:121`, `entered_rights`: `ModifyItems` added | unit `placement.rs:481` and `:415`, left `Bitmap 48`, right `Bitmap 16` | yes, both (unit only, R42-9) |

There is no J24.

### The controller's rows

| Id | Fault | First failure observed | Outcome |
|---|---|---|---|
| X1 | `za_commit_update`: `pairs = None` when the placement plan is a change and no message was built (a quiet move) | none at `f3507acdc`: the whole suite passed (1728 s) | survived; ruled a real gap; Task 36's fix round 2 (`de67af5df`) added two quiet moves to `move_with_scheduling_into_shared`; against `de67af5df` X1 failed at `sharing.rs:2129`, "event 30 holds no pairs" (inside `stored_pair`, in the first quiet move by the run order; no backtrace) |
| X2 | `increment_sequence` (upstream's `groupware/src/calendar/sequence.rs`) skips components with a RECURRENCE-ID | none: the whole suite passed (1834 s); M7b printed the sides equal (stored 6, items 6, mail 6) because the scheduling path raised the override itself | survived; an accepted residual in an upstream function, not an equivalent mutant (ruling above); run under ZA only, not JK or JP |
| X3 | the key writer's update raises every scheduling component's SEQUENCE once more after `increment_sequence` (written inline: the accessors are a private trait's; the first form did not compile) | `tests/src/za/imip/oracle.rs:604`, oracle main step M2, the JSCalendar comparison: CalDAV side `"sequence": 1`, JMAP side `"sequence": 2`, everything else equal | killed; the cross-side comparison failing that R38-12 asked to see; M7b was not reached |
| X4 | `calendar_event/set/za.rs:1182`: the `increment_sequence` call of the update deleted (base `de67af5df`) | `tests/src/za/jmap_calendar/oracle.rs:1801`, in `round_trip_keeps_what_an_ordinary_account_keeps`: the key account's lines hold no `SEQUENCE` line where the ordinary account's hold `SEQUENCE:1`; the oracle's main, quiet and inbound steps all passed first (M2 stored SEQUENCE 1 on both sides) | killed (ZA, exit 101, 369 s; no JK run needed; `task-42-batch-E.md`). Every scripted step before it printed the sides equal, M7b included (stored 6, item 6, mail 6 on both sides): on a scheduled update the scheduling path raises SEQUENCE itself, so M7b does not protect the call; the oracle's Part A does, on an unscheduled edit, by setting the key writer against upstream's writer for an ordinary account. With X3 (a wrong extra raise, killed at M2) this is the X2 and X4 record ruled above; the override branch inside upstream's function stays X2's accepted residual |

### Follow-up runs

- **J4s.** J4's edit with the corrected ZA-short recipe (the four `setup::` calls kept, `disabled::test` through `sharing_races::test` removed): `events.rs:1444`, "update_keeps_pairs_and_display_name: the update dropped the token pairs", the named catcher. The first ZA-short form removed `setup::test` too and stopped in `tokens::test`'s own setup (`tests/src/utils/za.rs:181`, 409 "not a key account"), so it did not reach the catcher.
- **Corrected first attempts.** J3's first edit queued in a batch with no document set and failed for that reason ("No document is set for a document task"); it was corrected once and run again. J8's and X3's first edits did not compile and were not run.
- **X1 after the strengthening**, above.
- No row was rerun for a result; every kill was observed once. The runner sets no `RUST_BACKTRACE`, so a panic inside a helper names the helper's line; the review checked each attribution by inference (J1, J2, J5, J9, J11, J16, J16b, J21) against the test source and each holds. Unit runs (J8, J9, J15, J16, J16b, J17, J26) have no command log (R42-12).

### Survivors

X1 survived and was answered with a strengthened test, which kills it. X2 survived and is an accepted residual in an upstream function. X3 and X4 were killed. **Of the brief's 28 rows, no mutation survived.**

### Kill checks earlier tasks ran

Each was made against a task's source, reverted (from a saved file where uncommitted work had to survive, otherwise with `git checkout`), with an empty `git status --short crates/` afterwards; none was committed. Lines are as of each run's source.

| Task | Mutation | Caught by |
|---|---|---|
| 10 | `uid.rs`: the loop over `&calendar_ids[..1]` | `pipeline.rs:344`, "create_uid_taken_in_second_calendar [a, b]: inserted conflicting document 4" |
| 10 | `create.rs:255`: the commit path's lost-create fence over the first calendar only | `pipeline.rs:502`, "lost_fence_rechecks_every_calendar jmapcal-pipe-lost: Busy(CreateLost)" |
| 11 | the container reseal removed from the update | `dav_seal.rs:1299` from `pipeline.rs:898`: the grantee's read got a per-item 500 with no dead-property values; the owner's read at `:897` succeeded |
| 12 | the destroy's `send_itip` set to false | `pipeline.rs:1657`, `destroy_single_name_equals_dav_delete`, grantee new inbox items left 1, right 2 |
| 17 | R17-2's hand-off cleanup removed from `za_token.rs` | `tokens.rs:1561`, `classification_failure_removes_the_handoff`, "the hand-off outlived its code" |
| 26 | the `/queryChanges` handler form's rebuild removed (`changes/query.rs`) | `query.rs:1287`, "no rebuilt Store error was traced for both methods (1)" |
| 29 | `za_replace_alarms`: no removal; unlink instead of remove | unit: `former_alarms_removed_not_orphaned` and `no_defaults_leaves_no_alarm` fail under both |
| 31 | the `za_sequence_floor` call removed | `events.rs:1492`, "the stored SEQUENCE is not the floor", left 1, right 5 |
| 36 | the create's default-alert writer is the account id, not the personal id | `sharing.rs:1981`, `grantee_default_alerts`, "the second event's alarms" (the owner's `-PT10M`) |
| 36 | `pairs = None` on every placement change (`pipeline/update.rs:146`) | `sharing.rs:2209`, "the stored pair changed across the move" |
| 36 | X1 against the fix round 2 commit | `sharing.rs:2129`, "event 30 holds no pairs" |
| 37 | `CalendarEventNotification/set` served | `refusals.rs:734`, `notification_destroy_leaves_the_item` (no method error; upstream's handler answered the pending item `notFound`, so the item-gone half did not trigger, R37-3) |
| 38 | `ZaEventWriter::destroy`: scheduling off behaves as on | first `scheduling::cancel_and_decline_on_destroy` (an item without the flag); with that module's call skipped, `oracle.rs:1303` at `207b77bb1`, "quiet step Q2 (first CalDAV, second JMAP): JMAP: the new inbox items", left `[(Invitee, "CANCEL")]`, right `[]` |

Task 40's two negative controls caught their faults (junk in field 153: "field 153 does not parse as a token record"; an access secret appended to the record found in `Subspace::Property`). Task 22 executed no kill check; its report argues by reading which case fails for each removal.

### What has no row

- **D21** ("a write that only removes entries asserts the record alone") has no row: a removal reads no vault record, so the fault cannot be made without adding a read, and nothing would make that read stale (spec 9.7). `removal_write_asserts_the_record_alone` and `forget_grant_asserts_the_record_alone` test it.
- **D28** has no fault in spec 9.7's list. Its tests are `calendar_set_error_table` and `calendars::store_failure_stops_the_call` (Task 28), `event_set_error_table` (Task 30) and the two mail-guard cases (Tasks 31 and 33).
- Known equivalent at suite level, not run: the attachment download's denial at `jmap/src/blob/download.rs` (R37-12); the 404 stays with the denial removed, because the embedded reader finds no attachment in the sealed tree.

### Kill checks of the final review's fix wave

The six kill checks K1 to K6 (and K6b) of Task 44's fix wave are tabled under "The final review" below.

## The final review

### How it ran

Five independent reviewers read the whole branch at `5cadcd739` against the base `ade47a9ae`, each with a lens: A sign-in, the token record and the session rule; B the pipeline, the move out of `dav` and parity with CalDAV; C1 `CalendarEvent/set`, scheduling and default alerts; C2 admission, reads, `Calendar/set`, queries, paged sync and sharing over JMAP; D leaks across the change, the tests that changed, the gates and the documents. Five rather than one because the branch (118 files, about 42,800 added lines) is more than one reader can read closely in one context (ruling above). Their briefs held no earlier finding, report or ruling; the deferred lines and coverage notes of the task reviews were put to each lens only after its report, for a triage section. The review started before the matrix finished, with an extract of the rows already run (ruling above). Each reviewer read only and ran nothing.

### Totals

No Critical. Eight findings rated Important, six distinct (FA-1 = FD-2, FA-3 = FD-1). 78 Minor, with duplicates across lenses. One finding withdrawn by its own reviewer (FC2-4: a master login returns a token with no keys, as the ledger's R37-14 says).

### The six Important findings

- **FA-1 = FD-2: a second-factor change did not end sessions for good.** The binding is a function of the salt, the Argon2 parameters, the verifier and the second-factor secret, so removing a second factor restored the earlier binding and the old tokens authenticated again, keyless. Ruling: fixed; every change of the second factor draws a fresh salt. The wave: `za_totp` calls `za_set_password` with the password it has just verified and the master key it holds, in the record it commits (P1); the case `tokens::second_factor_change_ends_sessions` holds real tokens across second-factor changes (the factor removed, the same URL enrolled again) and a recovery (T1).
- **FA-3 = FD-1: the hand-off secret and the code reached the request-URL trace** when the client's redirect is served by this server. Ruling: fixed for every request, pending the owner's confirmation as an amendment of spec section 10. The wave: `traced_url` (`crates/http-proto/src/request.rs`) masks the value of every `code` query parameter, `traced_header` passes a `referer` through it, and the three URL sites of `crates/http/src/request.rs` format through it (P2); six unit tests and the case `tokens::traced_urls_mask_the_code` (T2).
- **FA-2: the D27 case could not fail** (fifteen refusals against a ban rate of 100 a day). Ruling: fixed as a test. The wave: the case lowers the ban rate to 5 for one forwarded address, makes six refusals and shows the address is not banned and a valid request from it is served (T3; kill checks K6 and K6b).
- **FC1-1 (the ledger's R36-10): a key account that creates an event as an attendee with the scheduling flag sends no reply**, and that copy never schedules afterwards; upstream's JMAP create falls back to the attendee's reply, CalDAV's does not. Ruling: no product change (it is what D1 asks for); a pinning case, and the owner decides between a line in spec 3.3 and an amendment of 3.2. The wave: `scheduling::attendee_create_sends_no_reply` (T5).
- **FC1-2: no test made a `CalendarEvent/set` create fail with a store error, and none crossed object kinds** (D28, invariant 75). Ruling: fixed as a test, with R32-1. The wave: `scheduling::mail_guard_on_failed_create` (a create's commit fails: the create before it keeps its commit and its mail, the failed create's mail is dropped, and every later object of the call, of any kind, gets the same answer unattempted), and `events::placement_refused` asserts the whole refusal (T4).
- **FC2-1: a grantee's text query matched the fields `CalendarEvent/get` hides**, for an event whose CLASS is sealed (a non-standard value). Ruling: fixed; for a caller that is not a member of the account an event whose unsealed tree is not public enters no text index (P3, `crates/jmap/src/calendar_event/query/za.rs`). The case `query::grantee_text_skips_a_sealed_class` (T6). The remaining difference from CalDAV (FC2-2) goes to the owner.

### The fix wave

One implementer, four commits on `7000924b4`:

- `3f1ce7dc5`: P1 and T1.
- `f1c69900d`: P2 and T2.
- `f0150ec57`: P3 and T6.
- `a5e45dc2f`: P4 (an empty context list refused with `Forbidden(Context)` at the head of `za_commit_update`, and by the controller's addition in `za_refresh_event`), P5 (the `OAuthGrant` arm of `crates/common/src/cache/invalidate.rs` also evicts the account's resident keys), P6 (a compile-time equality of `CODE_LEN` and upstream's `DEVICE_CODE_LEN` beside `is_server_code`); comments for FA-15, FC2-11, FD-12 and R37-14; the two evidence strings of `tests/qualification/dispositions.json` (FD-10 = R41-2, R41-3); and the cases T3 to T5 and T7 to T11 (T7 `sharing::revoked_and_unshared_calendars_hidden`; T8 the ordinary local attendee's delivery; T9 a leak scan while a hand-off entry is live; T10 `pipeline::update_without_contexts_is_refused`; T11 resident keys gone after a grant's revocation).

Its runs: TM and SHIP clean, six format checks clean, the six `traced_url` unit tests, vault 208 and groupware 59, `ci_steps.py` 0 differences, `test_gates.py` OK, and one whole ZA passed (1804 s), on the working tree before a two-line reorder in T1 (below); the matrix's second pass runs every row on the committed `a5e45dc2f`. Differences from its brief that its report records: T6 sits in `query.rs` beside `grantee_privacy`, not in `sharing.rs`, and has no ordinary twin (W-3); T1's removal step checks the tokens before the binding (the first K1 run failed at the binding comparison, and the commits were rebuilt once with that reorder); K6 as prescribed is caught by an older assertion, so K6b was added; `placement_refused` asserts no description, since the placement refusals are a bare `forbidden`; `traced_url` leaves a bare `code` without `=` unmasked and turns `code=` into `code=[redacted]`.

Cost the re-review recorded for the documents: after any grant deletion of a key account (P5), each Basic CalDAV client of the account pays one Argon2 verification on its next request and another browser session one record read; the grant arm evicts the key cache, not the search index cache.

### Kill checks K1 to K6

Each on the committed tree, with a short form of the suite (a block of module calls in `tests/src/za/mod.rs` removed, never committed), and the edited files restored after each (`git status` of `crates/` and `tests/` empty).

| Id | Change made | Case that failed | First panic |
|---|---|---|---|
| K1 | P1 undone (`za_totp` without the fresh salt) | `tokens::second_factor_change_ends_sessions` | "an access token was served after the removal", left 200, right 401 (the first run, before the reorder, failed at "removing the factor restored the earlier binding") |
| K2 | the third URL site formats the URL as before | `tokens::traced_urls_mask_the_code` | "a code value appeared in a trace" |
| K3 | P3 undone | `query::grantee_text_skips_a_sealed_class` | "the grantee's match of {"title":"cloakedcanary"}", left `["l"]`, right `[]` |
| K4 | P4's guard in `za_commit_update` undone | `pipeline::update_without_contexts_is_refused` | "index out of bounds: the len is 0 but the index is 0", in `placement.rs` |
| K5 | P5 undone | `tokens::sign_in_gives_keyed_tokens` | "resident keys outlived the grant's deletion" |
| K6 | the Bearer arm's refusal counted as an authentication failure | `tokens::tampered_or_foreign_secret_is_401` | its older trace-count assertion ("zero-access: token keys unavailable", left 0, right 9), before the new ban assertions |
| K6b | the refusal counted for banning without changing its error | `tokens::tampered_or_foreign_secret_is_401` | "fork refusal 5 from one address", left 429, right 401 |

### The re-review

A fresh reviewer read the wave's diff: every item addressed (P1 to P6, the four comments, the data file, T1 to T11), no new Critical or Important breakage, the kill checks shown from their logs, the tree restored. Its five Minor findings, ruled by the controller with no second fix wave:

- **W-1** (deferred, with the fix named): a request that passed authentication before a grant's revocation can make that grant's keys resident after the eviction; they are unreachable and stay until the idle or age limit. Fix for a follow-up: insert the keys only when the authentication entry went in.
- **W-2** (residual): T7 cannot fail on the fork's fresh-context filter, which sits behind upstream's ACL masks in every state a test can build without a hook; the case stands as coverage of the answer a grantee gets, and its comment overstates.
- **W-3** (deferred): T6 has no ordinary twin.
- **W-4** (deferred): T2 exercises one of the three URL sites; the helper has six unit tests.
- **W-5** (residual): `traced_url` masks a `code` query parameter as a standard parser reads one; a callback URL percent-encoded inside another parameter's value and a `;`-separated query pass unmasked. The server generates neither.

### The disposition of every finding

- **Fixed in the wave**: FA-1 = FD-2; FA-3 = FD-1 (pending the owner's confirmation); FA-2; FC1-2, with R32-1; FC2-1; FB-1; FA-4; FA-10 (with ledger 478); FA-15; FC2-11 (its comments; its list of copied checks goes to the documents); FD-12; R37-14's comment; FD-10 (= R41-2, R41-3); FC1-11; FC2-20; FD-8; FA-7 = FD-6 (by FA-1's case).
- **Corrected in the documents**: ledger 477 (the device-code poll's changed answer, also for the owner); FA-12 (the sweep's per-account read, also for the owner); FA-21 (tokens refused once after the upgrade); FD-14 (spec 2's delta for an operator); R35-5's wording (also for the owner); FC1-10, FA-9 and FC2-12 (the repeated upstream checks and the larger insertions, named as silent dependencies; the code stays, a residual).
- **For the owner** (the code stays until he rules): FC1-1 (with its pinning case); FC2-2 (a line in spec 3.3); FC2-3 = R34-4; FC2-5 (D15); FC2-6 (a default alert's id in the clear); FC2-15 (an empty `shareWith` refused; in the checklist); FD-3 (a reminder's push names a positional alert id; a line in spec 3.3); FC1-5 (a grantee's email default alert is mailed to the owner); FC1-4 (no change recommended); FA-6 = R18-8 (D8's sentence); FA-17 = R18-2 (4.5 step 5's wording); FB-3 (the placement UID check outside the lease; spec 3.3's wording); FB-5 = R33-5, with FC1-9 (an event that does not unseal answers three ways); FC1-16 (spec 8's row for missing access); FB-6 (a move out of two unshared calendars into one shared one); R18-6 with FA-13 (token refusals without a trace; a store error on the Bearer path answered 401); R28-4 (a committed calendar destroy answered "try again" on a cluster with a broken broadcast).
- **Residual**: FA-11 (an ordinary account's consent token with a 43-character suffix accepted); FA-14 (the sweep runs only after the purge steps before it succeed); FA-20 (D23 on a node that missed an invalidation broadcast); FB-2 (a create without a `uid` retried under a new UID after its fan-out); FB-7 (the assertion order of the placement and destroy batches, CalDAV's at the base); FC1-6, FC1-7, FC1-8, FC1-12, FC1-13, FC1-14, FC1-15 (the event writer's answers or order beside upstream's, as the plan prescribes, nothing stored in the clear); FC2-7, FC2-8 (an unranged text query's views held for the request; the leaf limit checked after the candidates open); FC2-9 (one shared calendar that does not open fails a grantee's `Calendar/get` and exact-origin `/changes`); FC2-10, FC2-13, FC2-14, FC2-17 (`Calendar/set`'s context before validation; non-canonical `z`; an owner's get that completes a boundary; an owner's sync completes none); FD-13 (store errors on the issue and refresh paths keep their context); FD-16 (`za_handoff_take` deletes before it checks the secret).
- **Deferred**: FA-5 (the D22 precheck and an unreadable record); FA-8, FA-18, FA-19 (test gaps on the token paths); FA-16 (one exit of `delete_oauth_grant` skips the entry removal; bounded by the sweep); FB-4 (the placement arm's exits trace "abandoned" where the outcome is known); FB-8 = R38-6, FB-9, FB-10 (test weaknesses); FC1-3 (a repeated update of one event in one call); FC1-17 (thin coverage of the occurrence-patch rulings); FC2-18 = FD-4 = R21-12, FC2-19, FC2-21 = R28-7, FD-5, FD-7 = R40-7, FD-9 (test weaknesses); FD-11 (new fork tests in `parser_restored_tests.rs`; to the gates' run); FD-15 (`ZaPipelineError` derives `Debug`; no use site formats it).
- **Inherited** (upstream's behaviour for every account, one line each): FA-22 (the device-authorization answer is traced with its device code; a retry while a rotation is pending returns the next generation); FC2-16 (JMAP destroys the default calendar that CalDAV's DELETE refuses in a product build; also a line in spec 3.3 for the owner); and from the lenses: an owner's JMAP update of alarms, keywords or colour only runs unscheduled while CalDAV's bumps the schedule tag; a CalDAV DELETE by a non-owner tests ownership on the stored tree; a large collection delete can commit in part after its cancellations went out; an unanswered attendee's JMAP create never schedules its copy; the first of two updates of one id in a call is lost; a grantee's destroy in an unshared calendar answers with the calendar's id; `/queryChanges` with `expandRecurrences` answers `cannotCalculateChanges` before the access check; a grantee's `/changes` reports updates of events it cannot see as destroyed ids; a `z`, like upstream's `r`, is bound to no account, collection or method; a windowed or property-subset read and a full read show default alerts differently.
- **Withdrawn**: FC2-4.

The deferred lines of the task reviews: no lens found one that requires a change of product behaviour before the merge. What they asked to be closed is ruled above (R36-10, R30-1 with R32-3, R33-1 and R32-1, R41-2 and R41-3, ledger 477, R35-5, R34-4, the D15 ruling, R37-14, R28-4). The run they asked for, ZA with the filesystem blob store, passed as the matrix's row 10; the inventory and coverage gates wait for the owner's word.

## Other harnesses

Task 41's records, all first runs, on `0237a0b50` (Task 41's base, which holds Task 39's fix `8cf35e32c`) plus Task 41's two edits:

| Label | Result | Time |
|---|---|---|
| WK | `test webdav::webdav_tests ... ok` | 24.91 s |
| IK | `test imap::imap_tests ... ok` | 13.65 s |
| JK | `test jmap::jmap_tests ... ok` | 183.49 s |
| PRODUCT (thin LTO, 16 codegen units) | exit 0, no warning; version line names source `0237a0b50` and `dirty` (the task's two edits were uncommitted at build time) | 5 m 27 s |
| the fresh-store unit suite | 9 tests OK | 7 s |
| FRESH | `PASSED: 9 readback checks after the restart` | 10 s |
| CALDAV, key and plain | not run: `python3.12` is not on the host; nothing was installed; left to CI's `caldav-client` job | n/a |

`git diff origin/main...HEAD -- tests/src/webdav` is empty, so WK runs upstream's CalDAV suite unchanged on the head.

**The rerun decision.** No rerun was added to the key-mode JMAP step: `calendar::alarm` failed in none of five key-mode runs (Task 39's three counted runs and its fix round's, and Task 41's). The decision rests on local runs only; the first hosted-runner evidence is Task 44's. The step's description in `test.yml` changed (a comment only), and `ci_steps.py` is unchanged; `python3 -I scripts/tests/ci_steps.py` and `python3 -I scripts/tests/test_gates.py` passed.

**The cohort additions.** `tests/qualification/dispositions.json` gained 23 entries, one per fork runtime file release 6 added under `crates/` (the pipeline's ten files, `za_alerts.rs`, `za_text.rs`, `vault/src/tokens.rs`, `common/src/za/tokens.rs`, `common/src/za/calendar.rs`, `za_bearer.rs`, `za_token.rs`, `api/za_calendar.rs`, `za_read.rs`, `query/za.rs`, the two key writers and `changes/za_calendar.rs`), each with `runtime_excluded: false`. Why: `crates/dav/src/common/za.rs` is a file of the baseline cohort and the move takes its lines into `pipeline/admission.rs` and `pipeline/context.rs`; left out, those lines and the rest of the release's fork code would leave the measured cohort. The cohort grows from 53 to 76 files; the coverage fraction is unmeasured until Task 44's gate, and a shortfall there is answered with tests. Two evidence strings in that file are wrong (R41-2, R41-3, deferred).

## Adjusted assertions

Every existing assertion a task changed, each with a comment in the code citing the spec section. Lines are at `de67af5df`.

- **`tests/src/za/gating.rs`** (Tasks 21 to 26, 28, 30, 35). `:233-249`: a key account's session advertises `urn:ietf:params:jmap:calendars` and `:calendars:parse`, primary for the account (spec 6.10, D13). `:312-323` and `:344-362`: the key principal's calendar capability block, `mayGetAvailability` false, `accountId` null for a caller without access and the account's own for itself (spec 6.10). `:382-416`: the loop of eighteen refused rows now holds the six still refused (`CalendarEvent/copy`, `CalendarEventNotification/*`, `accountNotSupportedByMethod`); `:418-471`: the served rows, each asserting no `accountNotSupportedByMethod` or `forbidden` (spec 6.2; `ParticipantIdentity` per 6.9). Rows moved between the loops task by task as the plan's "Moving a refusal row" says.
- **`tests/src/za/websocket.rs:75-85`** (Task 21): the same socket's calendar call answers `forbidden` with `ZA_CALENDAR_KEYLESS` instead of `accountNotSupportedByMethod` (spec 6.1).
- **`tests/src/jmap/za_variants.rs`, `calendars`** (`:3530`): the served block and refusal loop that Tasks 21 to 30 kept there were replaced by Task 39 with the calendar block in upstream's order (below). **`principal_get`** (`:6002`, Task 35): the session's `primaryAccounts` gain the two calendar capabilities (`:6273`), the account object offers the calendar capability (`:6113`), and each principal shows its calendar block with `mayGetAvailability` false (`:6349`) (spec 6.10, D13).
- **`tests/src/za/sharing.rs:1906-1911`** (Task 27): the conversion carries the owner's colour in its sealed personal bundle, where release 4 asserted none (spec 6.7, D14).
- **`tests/src/za/app_password.rs:686-689`** (Task 17): a comment only: the exchange now issues a keyed grant and a token record, which the app-password revoke leaves in place (spec 9.6, D19).
- **`tests/src/za/setup.rs:785-802`** and **`tests/src/za/password.rs:158-160`** (Task 14): a key account's credential version is `za_session_binding` of the vault record, compared exactly, where it was the vault revision (spec D19); the password case's comment says the same.
- **`tests/src/utils/cleanup.rs`** and **`tests/src/utils/storage.rs`** (Task 14): the emptiness check exempts field 153 while its account exists (spec 9.6).
- **Task 39's tables** (`za_variants.rs`; upstream's line numbers in each body). Taken from the code and the corrected tables (`c3fa2099c`, `0237a0b50`), not from the report's row count:
  - `calendar_event` (`:3563`, mirror of `calendar::event::test`): `:85-90` the create with `isDraft`, `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` true is refused `invalidProperties`, then created without them (6.5); `:154-160` the flags read false (6.3); `:169`, `:175-187` event 2's `useDefaultAlerts` false and its alerts the value `events::create_use_default_alerts` pins (D11); `:237-240`, `:250` the same (6.3, D11); `:280-313`, `:475-506` the override patch `alerts/abc/iCalendar` in windowed and property-subset reads (D11, D1; ruling F1); `:350-370` the duplicate UID created in calendar 1 and refused in calendar 2 (3.3); `:467` `useDefaultAlerts` false after the patch (D11).
  - `calendar_identity` (`:4798`): the destroy of `b`, the two foreign-address creates and the create with `onSuccessSetIsDefault: "#i0"` answer `forbidden`; the list keeps both identities; `onSuccessSetIsDefault: "b"` switches the default (6.9).
  - `calendar_notification` (`:4953`): each `CalendarEventNotification` method answers `accountNotSupportedByMethod` (6.11).
  - `calendar_acl` (`:5005`): `:146-161` John's `shareWith` refused `forbidden`, the grant made at `view` through the share endpoints (6.11); `:232-289`, `:399-455`, `:652-707` the share notifications with the rights of the level granted or revoked (6.2; ruling F2); `:294-301` Jane's empty patch at view `forbidden` (6.7; ruling F3); `:302-313` Jane's `shareWith: {}` `forbidden` (6.11); an added row: Jane's rename at view `forbidden` (6.7); `:357-369` the grant at `edit`; `:372-394` Jane's `myRights` at `edit` (no `mayDelete`, `mayShare`, `mayReadFreeBusy`, `maySetMetadata`) (6.7); `:474-525` `CalendarEvent/copy` `accountNotSupportedByMethod` (6.11); `:606-617` the revocation through the endpoint; `:710-736` the grant at `edit` and Jane's destroy refused "You are not allowed to delete this calendar." (6.7); `:738-744` the cleanup without `destroy_all_event_notifications` (6.11). Upstream's assertions at `:162-190`, Jane's reads of John's calendar, `:314-325` and `:330-353` stand unchanged (R39-1, R39-2).
  - `calendar_availability` (`:5792`): `:94-109` the `shareWith` grant refused `forbidden` (6.11); the two `Principal/getAvailability` assertions at `:127-177` and `:235-243` expect `[]` (6.2).

## Accepted residuals

- **Spec 3.3, in substance.** Each object of a `/set` is its own commit (D3); an unreachable or withheld recipient records its status and does not fail the object; an unexpanded text query is matched in memory; UID uniqueness is CalDAV's rule; destroying a calendar sends cancellations (D16); no drafts, per-user event data or participant-visibility flags; default alerts by value (D11).
- **Spec 3.4's four limitations**: a CalDAV colour and JMAP's `color` stay apart; an event written with `sendSchedulingMessages: false` and edited through CalDAV is scheduled from that edit on; upstream's conversion decides what survives a round trip; JMAP does not check a calendar's supported component set. Only the round trip has a test (`oracle::round_trip_keeps_what_an_ordinary_account_keeps`).
- **4.5's interrupted refresh.** If the grant advances and the record's write does not follow, the superseded secret and the stored record still open MK until the next write of the record, the client's retry or the sweep, at most one maintenance interval. Upstream refuses the superseded token itself at once.
- **D22's residual.** A keyed grant whose entries are gone while its own deletion failed (D20's eviction, 4.4's first write) refreshes as a keyless grant when its client presents the token without the secret. It opens nothing.
- **D7 at a cache hit.** D7 is decided at a cache miss: a node that has a token's keys cached serves it until that cache entry ends, even if the record's entry was dropped by the cap of four. A grant's deletion evicts the cached entry at once.
- **Section 2.** The token record is new at rest, and an entry can outlive its token until the next maintenance run (an expired token, a grant destroyed through the registry, an interrupted refresh). The hand-off stores MK wrapped under a secret inside the code for the code's lifetime. A browser now holds a credential that opens the account's keys for as long as its grant lives (upstream's default: thirty days without use). Whoever serves a web client's code can read what the user reads.
- **The three ordinary-account I/O costs** (spec 10): one cached account read at a code exchange, at a refresh and at a grant's deletion; one in-memory delete at a consent confirmation that carries no secret; cached classification reads in each calendar method, which spec 10 counts as one, while the code makes two for `Calendar/set` and `CalendarEvent/set` (admission, then the handler's head; R28-8, R30-3) and more than one for a calendar `/changes` request (R22-1). **D29's costs**: state-codec fields and context checks, with no global account lookup and no additional ordinary-account store I/O; key-calendar pages carry the origin fields and classify the account when choosing the continuation format. **The legacy grantee restart** (6.8): a grantee's legacy `r` continuation of a key calendar answers `cannotCalculateChanges`, and the client restarts from Initial.
- **4.3, a retried refresh.** An entry's expiry is computed when it is written, so on a retried refresh the next generation's entry can outlive its token by the retry's delay; the token's own expiry and the sweep bound it.
- **5.2.** Upstream stamps `updated` on an owner's update, so most JMAP moves ask `ModifyItems` as well; the two share levels cannot tell the difference.
- **Coverage that stays partial.** The refresh steps of 4.5 have a unit test for the classification only (`classify_refresh_cases`) and run otherwise in the za suite, where no case reaches the `Lost` or `Superseded` arm of the classifier (R18-4, N2). The pass-through of a foreign Bearer value (D26) has a unit test only.
- **R27-1, rollback.** Every binary built before release 6 refuses a coloured personal bundle (the opener at the base `ade47a9ae`, release 7 included), so a binary rollback below release 6, or a cluster of mixed versions, leaves a coloured shared calendar unreadable to the older binary.
- **X2.** The override branch of upstream's `increment_sequence` is covered by no test in this fork's net, for any account.
- **The final review's residuals** (by id; the review files hold each in full):
  - FA-11: an ordinary account's consent token followed by a 43-character suffix of the secret alphabet is accepted, the suffix stripped (also an every-account difference in `CLAUDE.md`).
  - FA-14: the token sweep runs only when the purge steps before it in the maintenance task succeed.
  - FA-20: a node that missed an invalidation broadcast answers D23's 401 to a request whose keys it opened at an older generation, until its cache entry ends.
  - FB-2: a create without a `uid` whose commit fails after its invitations went out is retried by the client under a new UID, so the attendees hold an event of a UID the organizer never stored (a line in the checklist).
  - FB-7: the placement and destroy batches assert in CalDAV's order at the base.
  - FC1-6, FC1-7 (= R31-2), FC1-8 (= R30-2, R31-4), FC1-12, FC1-13, FC1-14 (= R31-3), FC1-15: the event writer's answers and check order beside upstream's, as the plan prescribes; nothing is stored in the clear by any.
  - FC2-7 (= R21-8, R26-5) and FC2-8 (= R26-7): an unranged text query holds its opened views for the request, and the store's leaf limit is checked after the candidates are opened.
  - FC2-9 (= R21-5): one shared calendar that does not open fails a grantee's whole `Calendar/get` and its exact-origin `/changes`, where the owner gets `notFound`. **Weighs more than its rating** (lens C2): a browser client then loses the whole sharer account, not one calendar.
  - FC2-10, FC2-13 (= R22-3), FC2-14, FC2-17: `Calendar/set` reads its context before upstream's validation; a non-canonical `z` is accepted and echoed re-encoded; an owner's get can complete a sharing boundary; an owner's own sync completes none.
  - FD-13: store errors on the issue and refresh paths keep their context in the trace.
  - FD-16: `za_handoff_take` deletes the hand-off before it checks the secret, so a wrong secret consumes it.
  - W-2: `sharing::revoked_and_unshared_calendars_hidden` cannot fail on the fork's fresh-context filter, which sits behind upstream's ACL masks in every state a test builds without a hook (an equivalent mutant at suite level); its comment overstates.
  - W-5: `traced_url` masks a `code` query parameter as a standard parser reads one; a callback URL percent-encoded inside another parameter's value, and a `;`-separated query, pass unmasked. The server generates neither.
  - FC1-10, FC2-11 (its list), FA-9, FC2-12: the key writers repeat upstream's inline checks, and several fork insertions are larger than one call or a short branch; the code stays, and `CLAUDE.md` names them as silent dependencies and hotspots.

## Deferred findings

What stays open after this plan, by task: every `minor (deferred ...)` line of the ledger, with its id where the ledger gives one. The arrow names where the ledger routed it. The review files (`task-<n>-review.md`, `task-<n>-fix-review.md`) hold each in full.

The final review's triage found 23 of these lines no longer true at the head (fixed by a later task or the fix wave, or the owed record delivered), and they are removed: Task 17's length-constant line, Task 18's N1 and O1, R21-10, R22-4, R22-8, R23-4, R23-5, R26-8, R27-1, R29-7, R30-1, R30-9, R30-11, R32-1, R32-2, R34-5, R35-3, R37-14, R37-16, R38-12, R40-8, and the line of R41-2 and R41-3; R28-4 and R30-10 are rewritten to what still holds. A line marked **weighs more than its rating** is one a lens found heavier against the whole branch than at its task.

### Task 17

- (no id) A device-code poll whose `device_code` is the pending user code now answers `expired_token` for every account instead of `authorization_pending` (`token.rs:368-370`); no token was obtainable that way. Recorded since the final review among `CLAUDE.md`'s every-account differences, and proposed to the owner for spec section 10.
- (no id) A failed hand-off cleanup on the classification error is discarded without a trace (`za_token.rs:36`), as briefed.
- (no id) `PKCE_VERIFIER` duplicated in `tests/src/za/tokens.rs` from `tests/src/utils/oauth.rs`.
- (no id, evidence) The RED run's `tokens.rs` hash differs from the first GREEN and the committed one; the RED file is not preserved.
- (no id, evidence) The device-code guard has no observed failing run; a candidate mutation row.
- (no id, reviewer's out-of-scope note) Destroying a key account leaves its outstanding code and hand-off in the in-memory store until exchange or expiry (ten minutes).

### Task 18

- R18-2: a credential change before the refresh's first read answers `Lost` (grant deleted, "token keys lost" trace), not `VaultChanged`; spec 9.2 allows either.
- R18-3: run 2 of `refresh_crossed_by_credential_change` cannot catch an omitted vault assertion (the binding change clears field 153 in the vault write's batch).
- R18-4: no suite case reaches `classify_refresh`'s `Lost` arm, presents a well-formed wrong secret or reads the `Lost` trace; three-attempt exhaustion unexercised.
- R18-5: `delete_keyed_grant`'s failure text says "unissued OAuth grant not deleted" on the refresh path, and the `Lost` trace says "deleted" before the deletion is attempted.
- R18-6 (plan-mandated): `Superseded` and `VaultChanged` refusals emit no fork trace; spec 8 says a refused token open is traced (→ a question for the spec). **Weighs more than its rating** (lenses A and D): with FA-13, an operator has little to diagnose a failing browser session with.
- R18-7: a well-formed secret appended to a keyless grant's refresh token makes the precheck answer `Keyed`, and the `Lost` arm deletes that grant; the spec is silent.
- R18-8 (plan-mandated): an unusable token record makes each refresh delete its grant while D8's sweep leaves such a record alone (→ a mixed-version question for the spec).
- R18-9: parked refreshes hold a five-second client across Argon2 vault writes; passed under a one-minute load of 20 to 54.
- R18-10: the inherited linker warning and "tests (lib test) generated 1 warning" in the ZA and SYS logs.
- R18-11: `change_race_password` repeats `change_password`; `za_token.rs:123-126` repeats `:86-89`.
- N2: no ZA case reaches `classify_refresh`'s `Superseded` arm after the fix; the vault unit test covers the classifier.

### Task 21

- R21-3: no test protects the grantee filter in `calendar/get.rs:108-130` or the try-next-calendar arm in `za_read.rs:81` (→ Task 36 or 42; still open after Task 36, see R36-4).
- R21-4: the R21-1 regression subscribes to `DataCorruption` only and does not show that no other event carries the canary.
- R21-5 (plan-mandated): one unreadable shared calendar fails a grantee's whole `Calendar/get` where the owner gets `notFound`.
- R21-6 (plan-mandated): two 20-line inline blocks in upstream's `calendar/get.rs`; the re-trace arm nearly duplicates `za_read.rs:70-79`; import groups reflowed.
- R21-7: the handlers classify the account again apart from the boundary's decision.
- R21-8: the view memo never hits in `CalendarEvent/get` and keeps every opened view alive to the method's end.
- R21-9: the event row read follows the content read; a concurrent update can give a spurious trace and a transient `notFound`.
- R21-12 (plan-mandated): the served-methods loop in `gating.rs` passes on `serverUnavailable`.

### Task 22

- R22-1: three account classifications per calendar `/changes` request.
- R22-2: the grantee sees one calendar, so its `Calendar/changes` paging asserts nothing about continuation pages.
- R22-3: non-canonical `z` strings are accepted, so `oldState` echoes a re-encoding.
- R22-5: the `invalidArguments` text is defined twice (jmap-proto `parser.rs`, jmap `za_calendar.rs`).
- R22-6: no test reaches the in-process `z` arms at `changes/get.rs`, `changes/za.rs`, `participant_identity/changes.rs`.
- R22-7 (→ Task 43 or a documentation commit): I §7.1's call-site list and the header's `za_variants` row no longer match the code (recorded under "Decisions and remaining qualification").

### Task 23

- R23-1: `let mut` edits an upstream line in `participant_identity/set.rs:52`.
- R23-2: the moved gating rows' comment cites spec 6.9 only; the header asks for 6.2.
- R23-6: no case covers a grantee's `/set` on the owner's account reaching upstream's `reject_identity_changes`.

### Task 24

- R24-1: no test of a grantee parsing the owner's blob under its own account id.
- R24-2 (brief-mandated): the inbox emptiness check is absolute; a before/after delta would be sturdier.
- R24-3: "the method writes nothing" is not asserted.

### Task 25

- R25-1: no test covers the `Unresolvable`/`Corrupt` to `UnexpectedError` mapping.
- R25-2: every test uses English.
- R25-3: no row covers the Owner field's second source (an attendee with an owner role).
- R25-4: the unzeroed `ContentRecord.terms` and verbatim term keys are request-lifetime plaintext in process memory, allowed by invariant 60.

### Task 26

- R26-1: a 38-line inline block in upstream's `query.rs` repeats `query/za.rs`' view-opening steps.
- R26-4: nothing checks that a query without a text condition opens no event.
- R26-5 (→ the final review): an unexpanded text query keeps every opened event's full plaintext view for the request.
- R26-6: two sequential store reads per candidate; a networked store could pass the client's five seconds on a large calendar.
- R26-7: a query over the store's text-leaf limit is refused only after every candidate is opened.
- N1: expanded parity compares starts only, and two fixtures share a start.
- N2: the expanded loop never asserts a non-empty answer.
- N3: the expanded form asks the flat ANDs only as one multi-property condition.
- O1: the rebuilt-count check does not attribute events to methods; the canary assertion's sensitivity on that path is unexercised.
- O2: the negated expanded form is checked for one negation only.

### Task 27

- R27-2: no golden vector pins the bundle layout spec 11 depends on.
- R27-3: no unit test pins `open_shared` refusing a colour.
- R27-4: that rollback keeps the colour is shown by reading only.
- R27-5: the report names the wrong za module for the changed assertion.

### Task 28

- R28-3: `za_write_default` reads the previous default after `za_delete_calendar` may have cleared it; one edge differs from upstream, the effective default is the same.
- R28-4 (→ the owner, after the whole-branch review): `za_commit_delete` submits the mail and then returns the grantees' invalidation error after the deletion committed, so a committed calendar destroy is answered D28's "try again", later destroys are skipped and `newState` does not carry the commit. The review found it unreachable on one node; it needs a failing `invalidate_caches`, that is a cluster whose broadcast fails. **Weighs more than its rating** (lens C2): against invariant 75's "objects already committed keep their answers".
- R28-5: after a failed post-commit read of the stored default the fallback can report `isDefault: true` wrongly; two traces mislabel.
- R28-6 (brief-internal): create checks `shareWith` before metadata, update the reverse; both answer `forbidden`.
- R28-7: coverage gaps: no create/update commit or default-write store-failure case, no D16 decline or denied-status case, invariant 58's clear fields unchecked, no `Calendar/set` metadata refusal case, `grantee_destroy_refused` reaches upstream's refusal.
- R28-8: ordinary accounts get two cached classification reads per `Calendar/set`.
- F1: `calendar.clone()` copies the whole unsealed calendar for `accept`, kept for parity.
- F2: the head store error is rebuilt with "calendar read failed" details on a write method.

### Task 29

- R29-1: the `SUMMARY:Mail` canary assertion passes from the VEVENT's own SUMMARY.
- R29-2: the parser drops an empty SUMMARY, so the empty-summary filter is never exercised.
- R29-3: no VTODO case and no nested-VALARM case.
- R29-4: that the root still references both VEVENTs is unchecked.
- R29-5 (brief-mandated): the tests exercise upstream's merge; the fork's choices are untested.
- R29-6: the overflow `false` return comes after the edit has started (unreachable).
- R29-8 (→ the owner): replacing alarms by value drops acknowledged or snoozed state that upstream keeps.
- R29-9: the brief adds a member fallback the spec lacks.
- R29-10: the SHIP, TM, FMT and mutation logs lack commands, source hashes and the mutation diffs.

### Task 30

- R30-2 (plan-mandated): the disk-quota and UID checks run in another order than upstream's.
- R30-3 (plan-mandated): two classification reads per call for every account.
- R30-4: `create_two_objects_one_fails` cannot tell per-object commits from one batch.
- R30-5: no case checks the UID in a non-first calendar; several cases skip spec 9.2's CalDAV readback.
- R30-6: the unreachable missing-UID arm is a check of the fork's own.
- R30-7 (plan-mandated wording): the reworded comment at `set.rs:1890` is inaccurate for `CalendarEvent/copy`.
- R30-8: the store-failure trace says "write failed" for read failures too.
- R30-10: the head of `CalendarEvent/set`'s key writer reads the account twice when the call has creates.
- R30-12: with duplicate `useDefaultAlerts` keys any true wins.

### Task 31

- R31-2: one occurrence's ruling refusal fails every sibling occurrence, including a destroy.
- R31-3 (plan-mandated): metadata on a synthetic id gets the fork refusal.
- R31-4 (plan-mandated): the quota check runs before scheduling, so a denied update answers `overQuota`.
- R31-5: the update path's D28 trace content is shown by reading only. **Weighs more than its rating**, with R33-1 (lens D): a regression that traced the original store error would pass.
- R31-6 (rest): the other base-patch rulings untested on update.
- R31-7 (→ Task 43 or a sheet commit): I §7.2 says a view error fails the method; in `/set`, D28 makes it per-object (recorded under "Decisions and remaining qualification").
- R31-8: review focus 1 shows a vault write happened, not that it was the pin's rewrap.
- R31-9: the JMAP/CalDAV comparison of key2's status passes if both are empty.
- R31-10: two refusal cases never read back.
- R31-11: the unreachable instance `ImportedValues` branch is kept without a comment.
- R31-12: the `hideAttendees` update refusal lives inside `update_uid_change_refused`.
- R31-13: no events case runs the owner-organized floor input any more.
- R31-14: the fix report's load figure does not match the preflight log.
- R31-15 (→ Task 38 or 43): the update tests origin against the owner's addresses while the floor and the sender use the sender's roles, so a grantee-organized event's JMAP update without scheduling gets no increment and no `updated` stamp; not examined with scheduling on.

### Task 32

- R32-3: D28 on the plan path has no case.
- R32-4: after the move back nothing reads the event as the owner.
- R32-5: `placement_and_content_together` cannot tell one commit from two (Task 11's case proves the single commit).
- R32-6: no case drives a grantee's placement change through `CalendarEvent/set`.
- R32-7: no case observes `content_changed`; either fixed value is equivalent at suite level.
- R32-8 (→ the whole-branch review): an event with two names in one calendar loses its second name on a wholesale `calendarIds` with the same set, with no vanished href; upstream folds names the same way.
- R32-9: `.filter(ZaPlacementPlan::is_change)` never drops a plan; one predicate or a `debug_assert` would guard divergence.
- R32-10: `Places::finish` checks only the 200 of `shares/revoke`.
- R32-11: the macOS linker note in suite output.
- R32-12: a local `stored_event` beside the field of the same name.
- R32-13: `events.rs` grows to 2233 lines; a listing is repeated.

### Task 33

- R33-1: the D28 trace of a destroy's store failure is not asserted; the event writer's create and update traces are equally unasserted. **Weighs more than its rating** (lens D; see R31-5).
- R33-2 (plan-mandated): the own-items leg and `may_write_own` cannot be reached through the product and have no test.
- R33-3: `InstancePlan::DestroyEvent` is newly live and untested.
- R33-4: `no_itip_task_ever` still deletes over CalDAV.
- R33-5 (→ the whole-branch review, then the owner as FB-5 with FC1-9): a destroy with scheduling of a future scheduled event whose sealed tree does not open is answered as the retryable D28 case and stops every later id; without the flag it succeeds.
- R33-6 (plan-mandated): a byte comparison cannot tell "not attempted" from "attempted and failed".
- R33-7: test hygiene (a helper defined twice, a repeated lookup, a keep-list use, a bare `created_id(0)` as an assertion).
- R33-8: the macOS linker note in every ZA and JK build.
- R33-9: `destroy` takes `&mut self` and mutates nothing; one name used three ways.

### Task 34

- R34-3: no case sends the triggering request with a Bearer token that carries a secret.
- R34-4 (→ the owner and Task 43; the final review's FC2-3): the hook runs before the `using` check, `allowed_capabilities` and every permission check, so a keyed token whose OAuth scope excludes calendars still drains when it names a calendar method (recorded under "Decisions and remaining qualification"). **Weighs more than its rating** (lenses A, C1 and C2): release 6 first gives a key account scoped tokens that carry keys, and CalDAV's drain has the same property from this release on.
- R34-6: the `CalendarEventNotification/get` reply is discarded.
- R34-7: the mail case does not assert one pending item before the query.
- R34-8: two RED receipts recorded different file hashes.
- R34-9: the macOS linker note; `lsof`'s "output may be incomplete" in the preflights.
- R34-10: comment accuracy in `scheduling.rs`.
- R34-11: the answer of key2's `Calendar/get` on key's account is discarded.
- R34-12: the fix round ran TM, ZA and FMT, not SHIP (tests only).
- R34-13: `clear_inboxes` asserts only that no pending item remains.

### Task 35

- R35-1: the filter `za_shown_capability(is_key, false, key)` is now a no-op, and its doc comment describes something it does not do.
- R35-2: the `&& !is_key` in upstream's expression has no comment.
- R35-4: no failing run for `mayGetAvailability` left true.
- R35-5 (inherited, medium confidence; → the owner and Task 43): `Principal/getAvailability` for a key principal in an ordinary group with calendars returns the group's busy periods while `mayGetAvailability` is false (recorded under "Decisions and remaining qualification"; README-dev's wording corrected after the final review). **Weighs more than its rating** (lens D): the documents said "an empty list".
- R35-6: a comment in `za_variants.rs` omits and narrates.
- R35-7: the macOS linker note; `lsof`'s warning.

### Task 36

- R36-3: the view grantee's create and alerts update assert the type `forbidden` only, where upstream words both.
- R36-4 (rest): R21-3 stays open; Task 21's concern (3) stays open (a grantee's `Calendar/get` `shareWith`, the clear fields of 6.7 and the description unasserted).
- R36-5: `grantee_default_alerts`' first event cannot tell the grantee's copy from the owner's live entry; the update path's writer argument has no grantee case; no ZA case sends `useDefaultAlerts: true` in an update.
- R36-6: `delegated_reply_carries_sent_by` counts sink mail by recipient and method, not UID.
- R36-7: `move_with_scheduling_into_shared` empties both inboxes wholesale.
- R36-8: `new_items_for`'s bool is always true; the first invitation's method is not asserted.
- R36-9: the report's load range does not match the preflight logs; one run's source is kept as a hash only.
- R36-11: `assert_account_not_found` has no in-helper control.
- R36-12: `grantee_destroy_sends_delegated_cancel` does not pin the property or the owner's address on the `SENT-BY` line.
- R36-13: the new case never checks the owner's or the grantee's inbox.
- R36-14: each step counts new pending items by UID, not all new items.
- (no id, re-review nits) `ids[1]` collected and unused; an `assert_ne!` without a message.
- R36-15: `stored_pair`'s panics name neither the case nor the step.
- R36-16: the case name and message prefix still say `move_with_scheduling_into_shared` though the loop's moves are quiet.

### Task 37

- R37-3: the invariant 66 read-back compares pending item ids only.
- R37-6: some out-of-scope slots stay unasserted for a key account (same generic parser path).
- R37-7: two rows assert the error type only where a fixed text exists.
- R37-8: no read-back after the refused `shareWith` update; a comment claims more than the check.
- R37-9: the keyless session check omits `urn:ietf:params:jmap:calendars:parse`.
- R37-10: `call_using`'s Bearer arm repeats `za_bearer_jmap` and drops the status.
- R37-11: one more temporary upload is left in the owner's account.
- R37-12: the attachment download's 404 stays with the rule it cites removed; an equivalent mutant at suite level.
- R37-13: the third impersonation form does not show in the test that the holder holds keys or shared access.
- R37-15: the metadata read-back uses a get without the metadata capability.
- (no id, re-review notes) the D29 case never sends a stale exact state as `ifInState`; the fixture `jmapcal-refusals-impersonate` needed a line in the fixture inventory (added to `CLAUDE.md`).

### Task 38

- R38-3: the organizer's `SCHEDULE-STATUS` in the attendee's copy is not read.
- R38-4: a JMAP reply's acceptance is caught only through a drain inside comparison 6's GET on the 500 ms client; the likeliest flake source.
- R38-5: "drained" at M8 and M9 can be satisfied by the oracle's own CalDAV GET.
- R38-6: an event left in both calendars at M3 can pass; the placement step does not tell a move from a copy.
- R38-7: `jscalendar_of` projects the first object only.
- R38-8: the tally line counts empty comparisons; it is not quoted here as "values compared".
- R38-9: Part B compares the pairs' recipients, not token values, and does not assert the list non-empty.
- R38-10: an unset items or sink in `Expect` is not asserted empty.
- R38-11 (spec half; → Task 43): spec 3.5's last sentence and 3.3/3.4 owe the record of Q1 and M2 (recorded under "Decisions and remaining qualification").
- R38-13: M9's reply takes its SEQUENCE from each side's stored event.
- R38-14: the METHOD line is not set against the message's MIME parameter.
- R38-15: M7b asserts only that the top-level `sequence` left the patch.
- R38-16: M7b's item and mail SEQUENCE are pinned only against the CalDAV side.
- R38-17: the per-step line prints the first SEQUENCE of each text; it is not quoted here as covering the override.
- R38-18: the module's header comment names neither M7b's exception nor the METHOD comparison.

### Task 39

- R39-3: several refusals assert only the type `forbidden`.
- R39-5 (→ Task 43 and the owner, with F1): three consequences of D11 are not in spec section 12 (recorded under "Decisions and remaining qualification").
- R39-7: the entry's comment says "in upstream's order", but availability runs before `principal_get`.
- R39-8: the F1 control's source was not kept, only its hash and output.
- R39-9: two observations with no change proposed.
- R39-10, R39-11, R39-12: the report's acl table lists three unchanged rows, says one read is "no longer listed" though its row is, and overstates "ends as upstream's body ends"; this note takes the adjusted assertions from the code.
- (no id, re-review notes) the report's head table shows a superseded hash; upstream's `calendar::instance::test` has no emptiness check of its own; `CLAUDE.md` and README-dev described key mode as running "the calendar refusals" (corrected by this task).

### Task 40

- R40-7: the trace capture's only control is that it is not empty, and it ends before the revocation and the password change. **Weighs more than its rating**, with R40-9 and R40-13 (lens D): the leak module is the release's only whole-flow trace and store net.
- R40-9: `secret_forms` has no unit control.
- R40-11: the report says 26 labelled secrets; the code builds 24.
- R40-12: the conditional deletion of the owner's upstream OAuth grant does not record which branch it took.
- R40-13: `secret_hits` does not decode LZ4-compressed pickles above 8 KiB, has no URL-safe embedded form and reads the logical store only (an evidence limit).
- R40-14: `leak.rs`' header comment does not mention the new rule.
- (no id, re-review notes) R40-9 carries more weight; the post-session scans assert only `token_records == 0`.
- R40-15: the comment at `leak.rs:1894-1896` and the report give the wrong character count for a 32-byte value's slices; the behaviour is right.
- R40-16: the invitation's own colour is not read back.
- R40-17: the task canary is searched in base64 and hex only after the task is destroyed.
- R40-18: `events >= 3` where the count is exactly 3.

### Task 41

- R41-1: the report omits the product binary's version line.
- R41-4: the PRODUCT, fresh-store unit and FRESH receipts record no command, environment or HEAD.
- R41-5: the plan's step 1 script ignores dispositions and does not test equivalents or rename targets.
- R41-6, R41-7: the rerun decision rests on local evidence only; the report does not table the five runs. **Weighs more than its rating** (lens D): the key-mode JMAP step has no rerun and now runs upstream's timing-sensitive `calendar::alarm`; the first hosted-runner evidence comes with the pull request.
- R41-9: the new evidence strings drop the backticks the existing entries use.

### Task 42

- R42-2 (→ this table): J7's named catcher cannot catch J7; the expanded assertions of `two_accounts_same_answers` are the only catcher.
- R42-3: J3's kill is the indirect doubled mail.
- R42-4: J16b's suite run, J19 and J25 each failed in an earlier case of the same property.
- R42-5: for J10 and J11 the failing loop iteration is not in the log.
- R42-6: J14's kill is the gate's refusal answer, not the item-survival assertion.
- R42-7 to R42-12: J8's mutant is the brief's stronger form; the runner sets no `RUST_BACKTRACE`; J26 is unit-only evidence; J18's arm is slightly broader than the row; J1 keeps the lease and skips only the UID check; unit runs have no command log, and one batch remark about an unused import is wrong.

### For the whole-branch review

The ledger's coverage notes (⚠️) that stay open, for Task 44's reviewer:

- Task 18: no run traces a refresh, so D24 on a refresh and the `Lost` trace are unobserved; R18-1's window is wider on networked stores than on local RocksDB.
- Task 21: the payload-free boundary rebuilds Store errors only.
- Task 22: `/queryChanges` origin forwarding has its runtime evidence in Task 26; upstream's paging of `/changes` after a log trim is inherited unchanged (open item 4 below).
- Task 26: cost figures are local RocksDB, one node, debug build; that an external search store receives no term is by reading `dispatch/search.rs` only.
- Task 28: `za_delete_calendar`'s post-commit error reachability (R28-4) and `za_repair_owned_context` were not read.
- Task 30: the email-alarm cleanup is shown only by the closing `wait_for_tasks`; the lease race is exercised at one interleaving (`COMMIT_PAUSE`).
- Task 31: after the fan-out, `za_commit_update`'s own error returns end the mail guard as `ZA_IMIP_ABANDONED` rather than `ZA_IMIP_DROPPED` (pipeline code); upstream's draft gate on scheduling and its hidden-participant path are not mirrored (they rely on no key event carrying those flags).
- Task 32: the placement UID check holds no lease and no batch fence (`pipeline/update.rs`), so a concurrent create or move of the same UID into the same calendar can both commit, while spec 3.3 says "checked under the UID lease" (true of creates only); on the plan retry `za_placement_contexts(..).await?` returns without `za_mail.dropped()` (the no-plan branch has the same shape).
- Task 33: a destroy in the same call as a failing create or update is shown by reading only.
- Task 36 (fix round 2's re-review): narrower mutants still survive by reading: a quiet link or unlink of an event with attendees, a quiet move between two unshared calendars of the same domain, a grantee's quiet move, recurring or multi-attendee events, the commit's bounded retry on a quiet move. X1's stated consequence (a later reply refused) is never observed, and the second quiet move has no kill evidence of its own.
- Task 37: the strength of the `stateMismatch` assertion against a fault that takes `to_id` as the state for calendars is unconfirmed without a run (R37-1's control shows it).
- Task 38: R31-15 is not examined by the oracle, which has no grantee step; the oracle covers no sharing, alerts or queries.
- Task 40: no earlier scan meets a live token record, so the field-153 rule's only live exercise is `leak::test_jmap_calendars`; spec 9.5 words the iTIP-task rule as a scan rule "for every scanned account" while the code checks it in the module.
- Task 41: the CalDAV client was not run in either mode, so that leg of spec 5.1's proof of the move stays open until CI; the inventory and coverage gates run in Task 44's CI; no receipt records `git status` during the runs.
- Task 42: the unreached named catchers are not shown to fail under their mutants; a clean tree after each row rests on the hash check and the batch-end status; X2 ran under ZA only; the unscheduled JMAP update that omits `sequence` on an event with an override is compared by no test (X2's residual).
- R32-8 and R33-5 (above): the review found R32-8 still true and latent (upstream folds names the same way) and sent R33-5 to the owner.

### The final review

Deferred by the final review and its re-review (by id; the review files hold each in full):

- FA-5: the D22 precheck's answer when the token record cannot be read.
- FA-8, FA-18 (= R18-3), FA-19: test gaps on the token paths.
- FA-16: one exit of `delete_oauth_grant` skips the record entry's removal; the sweep bounds it.
- FB-4 (with Task 31's and Task 32's coverage notes): the placement arm's exits end the mail guard as `ZA_IMIP_ABANDONED` where `ZA_IMIP_DROPPED` is true; nothing is mailed either way.
- FB-8 (= R38-6), FB-9, FB-10: test weaknesses in the oracle and the placement cases.
- FC1-3: a repeated update of one event id in one call.
- FC1-17: thin coverage of the occurrence-patch rulings.
- FC2-18 (= FD-4 = R21-12), FC2-19, FC2-21 (= R28-7), FD-5, FD-7 (= R40-7), FD-9: test weaknesses.
- FD-11: new fork tests sit in `parser_restored_tests.rs`, which the gates' tooling groups as restored upstream tests; for the gates' run.
- FD-15: `ZaPipelineError` derives `Debug`; no use site formats it.
- W-1: a request that passed authentication before a grant's revocation can make that grant's keys resident after the eviction; they are unreachable and stay until the key cache's idle or age limit. The fix for a follow-up: insert the keys only when the authentication entry went in.
- W-3: `query::grantee_text_skips_a_sealed_class` has no ordinary twin.
- W-4: `tokens::traced_urls_mask_the_code` exercises one of the three URL sites; `traced_url` has six unit tests.

## Merge hotspots

`CLAUDE.md`'s paragraphs "Release 6 added merge hotspots" and "Release 6 silent dependencies" hold the full list: spec 11's two lists with Task 43's additions. In short:

- **Fork insertions in upstream files** (spec 11): `crates/http/src/auth/oauth/{auth,token}.rs`, `crates/common/src/auth/oauth/{token,grant}.rs`, `crates/http/src/auth/authenticate.rs`, `crates/common/src/auth/{vault,access_token}.rs`, `crates/types/src/field.rs`, `crates/services/src/task_manager/tasks/maintenance.rs`, `crates/jmap/src/api/{request,session}.rs`, `crates/jmap/src/principal/get.rs`, `crates/jmap/src/changes/{get,query}.rs`, the two `/set` handlers' head branches, the two `/get` handlers, two points in `calendar_event/query.rs`, `participant_identity/{set,changes}.rs`, the shrunken `dav` key branches, `crates/{dav,jmap}/Cargo.toml`, and D29's `crates/jmap-proto/src/{types/state,request/parser}.rs` and `changes/za.rs`.
- **Added by Task 43**: upstream's `changes/get.rs` body moved unchanged into `ChangesPage` (R22-4); the key writers repeat the heads of upstream's `/set` handlers; `za_itip_fails_write` names every `ItipError` variant; `za_split_bearer` relies on `TOKEN_HEADER`; D29's codec, context checks, origin preservation, legacy owner chain and `/queryChanges` forwarding; D11's recipient and description rules copied from `alerts.rs`; the query branch's reliance on no `EVENT_HIDE_ATTENDEES`; each use of `za_session_binding`; the pinned 404 text (R37-16); the oracle's dependence on upstream's converter; the calendar variants of `za_variants.rs` (R44); R27-1's rollback.

## Decisions and remaining qualification

**Approved.** D19 (decided 2026-11-14) and the explicit-origin D29 amendment (approved and recorded 2026-11-15) are approved; origin inference is not reopened.

**D29's evidence.** Task 22 (handler level, local RocksDB): `paged_sync_retention_collision` reproduces the retention-marker collision through the real HTTP handlers with a log truncated by the existing `truncate_change_log` API in setup, not by the maintenance timer: the first pages carry distinct origins, and after a rename the initial-origin page is served and the exact-origin page answers `cannotCalculateChanges`. `paged_sync_origin_survives_range_changes`, `paged_sync_legacy_and_ordinary` (a grantee's legacy `r` restarts from Initial; a key owner's `r` chain stays `r`; ordinary accounts' states byte-equal to upstream's), `calendar_state_slot_errors` and `precondition_matrix` (out-of-scope `z` is `invalidArguments` in a state slot and `stateMismatch` in a precondition slot) and the codec and parser unit tests (`za_calendar_state_*`, `za_calendar_state_contexts`). Task 26: `query_changes_paged_origin` pages `/queryChanges` from Initial and from an exact state, three pages or more, every intermediate state a `z` with its origin unchanged; exact-origin continuations are refused while a boundary is pending and after a rename, initial-origin ones served; legacy and ordinary forms as above. This is source and handler evidence; it is not the reference client's result. The checklist's D29 rows (`docs/zero-access/client-jmap.md`) are that separate gate, and they have not been run.

**New unresolved findings.** None was Critical or Important at its task's gate after the fix rounds. The open items below are each a ledger line.

**Observed status of the costs the brief names.** The memory of an unranged text query was not measured: the query holds every candidate's terms, and by R26-5 every opened event's full plaintext view, for the request (spec 6.4 accepted holding the terms). Its time is review focus 5's. The `/set` answer after D11 is upstream's, as spec 6.5 says: `events::create_use_default_alerts` asserts that a create sent with `useDefaultAlerts: true` answers with neither `alerts` nor `useDefaultAlerts` (`events.rs:1158-1161`), and `events::create_reads_back_over_caldav` that a create without the flag answers `useDefaultAlerts: false` (`events.rs:960`); the alerts appear on the next `CalendarEvent/get`.

**Open items for the project owner.** None blocked a task. This list joins the task reviews' open items with the final review's; where the two overlapped, the final review's id is named beside the ledger's. Each carries the adjudication's recommendation.

To confirm:

1. **The `code` masking (FA-3 = FD-1)**, as an amendment of spec section 10: the value of every `code` query parameter is masked in a traced request URL and a traced `Referer`, for every request, a third every-account trace exception beside D24 and D26. Recommended: confirm; it is in the code at `f1c69900d`.
2. **D15 as a sync-correctness rule, not an access control** (ruling at Task 22; the final review's FC2-5, which shows the token that skips the boundary check, since a `z` state's origin is not authenticated). Recommended: confirm.

To decide:

3. **FC1-1 (= R36-10)**: a key account that creates an event as an attendee with the scheduling flag sends no reply, and that copy never schedules afterwards; upstream's JMAP create falls back to the attendee's reply, CalDAV's does not. The code is what D1 asks for; the case `scheduling::attendee_create_sends_no_reply` pins it. Two consistent ways out: add the line to spec 3.3 (recommended), or amend 3.2 so the scheduling input carries "a create by a non-organizer may reply", set by JMAP only, with one fallback in the pipeline's create arm (which CalDAV shares). Lens B's answer on what such a reply would reach: for a key organizer the fan-out would not attempt it (the copy holds no token pair, the organizer's status would record 5.3, and the drain drops a reply without a token); a mail-class organizer would be mailed.
4. **FC2-3 (= R34-4)**: the drain runs before the `using`, scope and permission checks, so a keyed token whose OAuth scope excludes calendars still drains (spec section 2 says a scope limits what the server will do for the token). The drain does only what the account's next keyed calendar request does, and the token chooses nothing. If the scope is to be honoured, the JMAP change is one condition; CalDAV's drain has no scope-filtered value at that point. Otherwise a line in section 2.
5. **FC2-6**: a default alert's id, a string the client chooses, is stored in the clear. Recommended: accept under 6.7 ("default alerts stay visible") with a line in the spec now; the key writer refusing or rewriting ids that are not short tokens as a follow-up.
6. R29-8: acknowledged or snoozed alarm state is lost with by-value default alerts.
7. R18-7: a well-formed secret appended to a keyless grant's refresh token deletes that grant.
8. Upstream's paging of `/changes` after a log trim is inherited unchanged and applies to ordinary accounts' continuations as well (Task 22's concern 1); pending the owner's decision, not accepted.
9. The two oracle rulings of Task 38: items and mail compared as the JSCalendar projection, and the script's explicit SEQUENCE with M7b leaving it to the server.
10. Task 39's F1 with R39-5: three consequences of D11 not in spec section 12. A constant description on every component would remove the first two and needs a change to D11's wording.
11. Task 39's F3 note: section 12 words the view-level consequence as "cannot rename or recolour"; the check also refuses `isVisible` and `isSubscribed`.
12. R41-8: the fresh-store restart script exercises nothing release 6 adds (no OAuth grant, no token record across the restart, no JMAP calendar call).
13. R31-15: a grantee-organized event's JMAP update takes a different path from its CalDAV twin (origin tested against the owner's addresses, the floor and sender with the sender's roles); with scheduling on, the update still goes out with a raised SEQUENCE (lens C1), so the gap is confined to scheduling off.
14. R43-4: whether release 4's sharing spec and plan 14's outcome note get a pointer to D14 at their four statements of the colour exclusion (listed under known staleness below).
15. FC1-4 (an owner's per-user-only update stores the converter's whole output): no loss was established; recommended no change.
16. FB-6 (a move out of two unshared calendars into one shared calendar is refused, though two updates reach the same end state): recommended accept, or a follow-up.

**Proposed spec amendments, for the owner** (the spec file is not edited by this plan):

- **3.3**:
  - FC1-1 (if the first way out of item 3 is taken): an attendee's JMAP create sends no reply, and that copy does not schedule afterwards.
  - FC2-2: a grantee's JMAP read of an event whose CLASS is sealed is private (as `/get` and, since the fix wave, the text query treat it), while CalDAV serves it whole; field policy 2's recorded limit, not this release's.
  - FD-3: a key account's reminder push names a positional alert id (`k1`), since the alarm's id is sealed.
  - FC1-5: a grantee's email default alert is mailed to the calendar's owner (release 1's alarm task); or a line in 3.4.
  - FC2-16: JMAP destroys the default calendar that CalDAV's DELETE refuses in a product build (inherited).
  - The UID lease: "checked under the UID lease" holds for creates only (FB-3); a placement change checks against current archives without a lease, as CalDAV's MOVE does.
- **3.3 and 3.4, 3.5's last sentence** (R38-11): record that a JMAP write stores upstream's converter form (`DURATION` for `DTEND`, the export's property order), stamps `CREATED` and the update stamps, and raises SEQUENCE for a client that omits it, so its iCalendar text differs from a CalDAV client's for the same edit; the oracle compares at the JSCalendar projection for that reason.
- **Section 10**:
  - the `code` masking (item 1), a third every-account trace exception;
  - the device-code poll whose `device_code` is the pending user code now answers `expired_token` where upstream answers `authorization_pending` (ledger 477), and an ordinary account's consent token with a 43-character suffix is accepted with the suffix stripped (FA-11);
  - the costs: the token sweep's one read of field 153 per account in every purge run (FA-12); a pass over the key cache on any account's grant deletion (FA-4's fix); two cached classification reads for `Calendar/set` and `CalendarEvent/set` and three for a calendar `/changes`, where the section says one.
- **D8's sentence** (FA-6 = R18-8): a refresh over an unreadable token record deletes the grant, since it cannot rewrap a master key it cannot open; "a sign-in or a refresh writes over it" holds for a sign-in only.
- **4.5 step 5** (FA-17 = R18-2): a refresh crossed by a credential change before its first read is classified `Lost` and deletes the grant; spec 9.2 tolerates either answer.
- **Spec 8's row for missing access** (FC1-16): a grantee's update in an unshared calendar answers `notFound`, which reveals less than the row's `forbidden`. **8** also: whether `Superseded` and `VaultChanged` refusals are traced (R18-6).
- **6.2, 6.10, 6.11** (R35-5): "an empty list" holds only when the key principal is in no ordinary group with calendars; a key member of an ordinary group returns that group's busy periods.
- **Section 12 (D11)** (R39-5): add the `iCalendar` member and override patches in subset reads, the stale description after a rename, and alarms that stay after a later `useDefaultAlerts: false`.
- **Section 12 (6.7)**: "cannot rename or recolour its own entry" becomes "cannot change its own entry, its visibility and subscription flags included".
- **Spec 10's amendment list and the sharing spec's colour statement** (R43-4): the amendments do not list the sharing spec's statement that no entry's bundle carries a colour, which D14 replaces.
- **4.5** (R18-7): a keyless grant presented with a well-formed secret.
- **9.5** (Task 40's note): the iTIP-task rule is checked in the module, not as a scan rule for every scanned account.

**Follow-ups not taken by the fix wave**, each with the adjudication's recommendation:

- R18-6 with FA-13: three token refusals emit no trace, and a store error on the Bearer key path is answered 401 with its cause dropped. Recommended: a store error answers a server error and is traced. Not in the wave, because it changes what a client sees under a store fault.
- FB-5 (= R33-5) with FC1-9: an event that does not unseal is `notFound` on update, a store failure on a flagged destroy (stopping later ids on every retry) and destroyed without the flag. Recommended: treat it per object on destroy too.
- R28-4: a committed calendar destroy answered "try again" when the grantees' invalidation fails after the commit; reachable only on a cluster whose broadcast fails.
- FA-5: the D22 precheck's answer when the token record cannot be read.
- FA-16: one exit of `delete_oauth_grant` skips the record entry's removal (bounded by the sweep).
- FB-4: the placement arm's exits trace `ZA_IMIP_ABANDONED` where the outcome is known.
- FC1-3: a repeated update of one event id in one call.
- A guard in the vault commit funnel that refuses a binding change under an unchanged salt, which would make FA-1's fix a checked property of every vault write (the unit test `credential_change_clears_the_token_record` would need four rows changed).
- W-1 (resident keys after a revocation crossed by an authenticated request), with its fix named under "Deferred findings".

**Known staleness in the plan's own documents and in older records** (not corrected here; none of these files is in this task's list): R22-7 (the interface sheet's §7.1 call-site list and the header's `za_variants` row); R31-7 (the sheet's §7.2 says a view error fails the method; in `/set`, D28 makes it per-object); the plan header's routing lines (R41-2 and R41-3 in `dispositions.json` were corrected by the final review's fix wave). Release 4's documents still state the colour exclusion that D14 replaces (Task 27's note; ruling R43-4): the sharing spec `docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:148-150` ("no supported client sets a per-principal colour") and `:566-570` ("No entry's bundle carries a colour a client could set"), and plan 14's outcome note `docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md:439` and `:1445` (row T6-P: the shared format excludes the JMAP-only personal colour). Since release 6, each principal's colour of a shared calendar is sealed in its own personal bundle and the grant's conversion carries the owner's. Those approved records are not edited by Task 43.

**Remaining qualification.** The whole-branch review has run (above). Task 44's second matrix pass on `a5e45dc2f` is recorded under "Where things stand" when it finishes. Not qualified by this plan: the reference client's checklist (`docs/zero-access/client-jmap.md`, its D29 rows included), the independent CalDAV client in either mode (so that leg of spec 5.1's proof of the move stays open until CI), the Docker suites (`directory`, `automation_tests`), the inventory and coverage gates with the cohort's new figure (FD-11), CI on the branch, the native-client rows of the manual checklist, other store backends, a second node, and the account page's text about how long keys stay in memory, which describes password clients only (spec 12, a follow-up outside this repository). Pushing the branch, dispatching the gates and opening the pull request are the owner's to authorize. A passing suite here is not review approval.
