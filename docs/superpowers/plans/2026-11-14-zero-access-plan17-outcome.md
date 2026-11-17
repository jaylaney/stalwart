# Zero-access JMAP calendars and browser sign-in: plan 17 outcome

Written 2026-11-17 by plan 17's Task 43, before Task 44 (the final test matrix and the whole-branch review). Plan 17 (`2026-11-14-zero-access-17-jmap-calendars.md`, its interface sheet and the six group files beside it) builds release 6 from the spec `2026-11-13-zero-access-jmap-calendars-design.md`, revision 7 (decisions D1 to D29, invariants 57 to 76). A key account's calendar works from an unmodified JMAP client in a browser: an OAuth token issued through the authorization-code flow carries a 32-byte secret after a `~`, a per-account token record (principal field 153) holds the master key wrapped under keys derived from those secrets, and the HTTP layer opens the keys on every request that presents such a token. The JMAP calendar methods read and write through the same key-account pipeline as CalDAV, which moved from `dav` into `groupware::calendar::pipeline`.

What this note can claim. Every result quoted here is local: RocksDB on one node, debug test builds, the in-store blob store unless a line says ZA-FS, a locally built thin-LTO product binary for the fresh-store script. No other store backend, no second node, no native client and no browser client was run. A passing suite is not review approval, and a review approval is not CI. The macOS linker's `__eh_frame` note appears in every suite log and is environmental. The reference client's checklist (`docs/zero-access/client-jmap.md`) is a manual gate that has not been run.

## Where things stand

- Branch `release6-jmap-calendars`, cut from the plan's branch `za-jmap-calendars-design`, on `origin/main` at `ade47a9ae` (the base after the rebase before Task 1; the checkpoints after Tasks 9, 19 and 37 found the base unchanged, the last read through the GitHub API because `git fetch` failed in that session; the rebase point before Task 44 is Task 44's). Head before this note: `de67af5df`, 82 commits ahead of `origin/main`. Nothing is pushed.
- Routing actually used. The plan header and the spec name Sol implementers and Astra reviewers; that routing ran Tasks 1 to 16, Task 17's first implementation and review, and Tasks 19 to 21. On the project owner's instructions of 2026-11-15, recorded in the ledger, Task 17's fix round and Task 18 had an Opus implementer and Fable reviews; from Task 22 on, implementers were Claude Opus (contract code and test modules that need judgment) or Claude Sonnet (Tasks 34, 35, 41 and the mutation rows of Task 42), reviews of Tasks 22 to 31 ran on Claude Opus, and every task review and scoped re-review from Task 31's fix round on ran on Claude Fable (xhigh for identity, authorization, persistence, ordering and the oracle; high otherwise). Task 44's whole-branch review is to run on Claude Fable. The ledger's "in progress" line of each task names its implementer and reviewer. The plan header's routing lines were not edited.
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
  - Task 43: the commit of this note, the reference client's checklist, `CLAUDE.md` and the release 6 documentation.
- The final matrix is Task 44's. Each suite's result and time is added there.

  | Check | Command | Result | Time |
  |---|---|---|---|
  | formatting | `cargo fmt --all --check` | Task 44 | Task 44 |
  | compile check, shipping and tests | SHIP, TM (plan header) | Task 44 | Task 44 |
  | vault and groupware units | `cargo test -p vault -p groupware --features vault/test_mode` | Task 44 | Task 44 |
  | common units | `cargo test -p common --lib` | Task 44 | Task 44 |
  | http units | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | Task 44 | Task 44 |
  | workspace units | U-ws with `UNIT_TEST_FEATURES` | Task 44 | Task 44 |
  | za suite | ZA | Task 44 | Task 44 |
  | za suite, filesystem blobs | ZA-FS | Task 44 | Task 44 |
  | cache swap suite | SWAP | Task 44 | Task 44 |
  | CalDAV suite, plain and key | WP, WK | Task 44 | Task 44 |
  | JMAP suite, plain and key | JP, JK | Task 44 | Task 44 |
  | IMAP suite, plain and key | IP, IK | Task 44 | Task 44 |
  | system suite | SYS | Task 44 | Task 44 |
  | product build | full profile, `--locked` | Task 44 | Task 44 |
  | fresh-store restart | FRESH | Task 44 | Task 44 |
  | independent CalDAV client, key and plain | CALDAV | Task 44 (CI) | Task 44 |
  | CI workflow and the retention gates | `test.yml` | Task 44 | Task 44 |

## What changed and why

**Group A, the move (Tasks 1 to 9; spec 5.1, D2, D17).** The fork's key-account calendar steps left the `dav` crate for `groupware::calendar::pipeline`: admission and roles (`admission.rs`), read and write contexts (`context.rs`), the UID lease (`uid.rs`), the scheduling assembly and the status of D18 (`schedule.rs`), the create, update and delete commits (`create.rs`, `update.rs`, `delete.rs`) and the placement resolver with the event container's reseal (`placement.rs`). The functions moved with their bodies, traces, hook order, lease releases and mail-guard settlements; the CalDAV handlers kept their upstream steps and call the pipeline where their key branches stood, and `From<ZaPipelineError> for DavError` keeps CalDAV's answers. The proof is that the CalDAV tests passed without edits to their assertions: Task 9's proof run at `a985f16b8` passed global formatting, the product build (7 m 59 s, warning-free), the `groupware` and `vault` units (196 and 47), the workspace units (842 passed, 2 ignored), ZA (1495.32 s), ZA-FS (1409.10 s), SWAP (17.56 s), WK (21.12 s), WP (19.04 s), the pinned CalDAV client in key mode (its harness's 34 unit tests; 74 passed, 1 skipped, 2 xfailed) and the fresh-store script (9 of 9 readback checks after the restart).

**Group B, the pipeline paths only JMAP uses (Tasks 10 to 12; spec 5.2, 5.4, 6.5).** A create named in several calendars checks UID uniqueness under the lease in every calendar (Task 10). A JMAP update can change placement and content in one commit: `ZaPlacementPlan::jmap` resolves the old and new calendars with the rights CalDAV's `COPY` and `MOVE` ask, the event and its dead-property container are resealed in the target domain, and one physical batch carries the whole update (Task 11; `CalendarEvent::enqueue_full_at` factored out of `update_full_at` for it). The destroy of a whole event linked into several calendars runs through the shared delete commit with `za_delete_send_itip` (Task 12).

**Group C, tokens (Tasks 13 to 19; spec section 4).** The vault crate gained the token secret's codec, the derivations and associated data, the token record with its caps and reconciliation, the refresh classifier and the hand-off value, and `za_session_binding`, the 64-bit fingerprint of the credentials that D19 makes a key account's credential version (Task 13). Field 153 and its storage, with the record cleared in `vault_batch_write` exactly when the binding changes (Task 14). Issue, open and forget with the two assertions of 4.3 and the three-attempt bound (Task 15). `decode_access_token` strips a `~` suffix from every token that begins with the server's token header, the token endpoint's successful answers are untraced for every account, and the HTTP layer's Bearer arm opens keys from the record and refuses a stale generation (Task 16). The hand-off at the password step, the code exchange that issues a keyed grant, and the refresh precheck of D22 (Task 17). The refresh of 4.5 (Task 18). The maintenance sweep of D8 and the record's end with its account (Task 19).

**Group D1, JMAP reads, queries and admission (Tasks 20 to 27; spec 6.1 to 6.4, 6.6 to 6.9, D14, D15, D29).** The calendar refusal text of D25 and the admission table (Task 20). `CalendarEvent/get`, `Calendar/get` and `Calendar/query` over unsealed views, with store errors rebuilt payload-free at the boundary (Task 21, and the R21-2 amendment for every later served method). `/changes` with the sharing boundary of D15 and the explicit-origin `z` continuation of D29 (Task 22). `ParticipantIdentity`, the default switch only (Task 23). `CalendarEvent/parse` for the keyed owner (Task 24). The in-memory text index built from release 7's content part (Task 25). `CalendarEvent/query` and `/queryChanges` with text conditions matched in memory (Task 26). A shared calendar's colour in each principal's sealed bundle (Task 27).

**Group D2, JMAP writes, scheduling, sharing and refusals (Tasks 28 to 37; spec 3.1, 5.3 to 5.5, 6.5, 6.7, 6.10, 6.11, D3, D4, D10, D11, D16, D18, D28).** `Calendar/set` with its key writer as a child module of upstream's handler, a calendar destroy that sends cancellations, and `shareWith` refused (Task 28). Default alerts written by value into the sealed tree (Task 29). `CalendarEvent/set` create, update with scheduling, placement and destroy, each object its own commit, a store error answered on that object and every later one (Tasks 30 to 33). The drain runs from a keyed JMAP request that names a calendar method (Task 34). The session and principal objects advertise the calendar capability for key accounts (Task 35). The sharing and refusals modules (Tasks 36 and 37).

**Group E, closing (Tasks 38 to 44; spec 3.5, 3.6, 9.3 to 9.8).** The differential oracle runs one edit script through CalDAV and JMAP and compares the stored event's JSCalendar projection, the items and mail, the schedule statuses and the stored pairs' recipients (Task 38). Upstream's JMAP calendar tests run in key mode through `za_variants::calendars` (Task 39). Leak rules for the token record, token secrets and JMAP calendar writes (Task 40). The other harnesses, the coverage cohort's additions and the key-mode JMAP step's description (Task 41). The mutation sample (Task 42). This documentation (Task 43). The final matrix and the whole-branch review (Task 44).

## The session rule

Decided by the project owner on 2026-11-14 (spec D19; section 12, decision 4): a key account's browser sessions end only when a credential changes, that is on a password change, a recovery or a second-factor change. They survive every other vault write: the first pin's rewrap, the self-check's repair, a new recovery key, and an app password's creation, publication, withdrawal or revocation. Each such write still raises the vault revision, so resident keys are evicted and the next request opens them again from the token record; a request whose keys were opened at a generation that is no longer current answers 401 (D23), and the client's refresh then succeeds.

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
| the mutation sample (Task 42) | rows J16 and J16b, both killed | |
| this documentation and the checklist row (Task 43) | state the rule | |

## Rulings

Every `Ruling:` line of the ledger (`.superpowers/sdd/2026-11-14-zero-access-17-jmap-calendars/progress.md`, git-ignored), one entry each: what was decided, why, and what it costs if wrong. Where a later ruling corrects an earlier one, the corrected text is given and the replacement is said.

### Execution and routing

- **Trailers.** The execution session's trailer set was empty at first: commits carried their prescribed subjects without copied attribution or an invented session URL. Why: no trailer identity was prescribed. Cost: none.
- **Task 44's context.** The whole-branch review gets the pinned base and head, the accepted contract and validation-only evidence, without prior findings, implementer reports or these rulings. Why: the owner's rule of an independent clean-context review. Cost: the rulings stay controller material to adjudicate the review's findings.
- **Routing from 2026-11-15** (replaces the plan header's Sol/Astra routing for the rest of the plan, without editing it). First: one Opus implementer at a time, Opus reviewers (xhigh for identity, authorization, persistence, ordering and concurrency). Then, on the owner's instruction "have the review agents run on fable", the reviews run on Fable at xhigh with implementation on Opus; later implementers Opus or Sonnet for mechanical work, task reviewers as the work's class asks, Task 44 on Fable. Why: the owner re-routed the work after the previous implementer's provider stopped it. Cost: a routing line to correct, no code.
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

- **F1, a D11 row.** In a windowed or property-subset read, each override whose title differed from the master's when the defaults were written holds the patch `alerts/abc/iCalendar` with that title. Why: D11 writes the alarms per component as upstream's iCalendar view does, and a read is upstream's projection; an ordinary-account control (alarms with different descriptions over CalDAV) showed the same patch. Cost if wrong: a client sees an unknown patch path, and a CalDAV client shows the title the event had when the defaults were written. Open for the owner: a constant description would remove both effects and needs a change to D11's wording. Upstream's full read and windowed read render the same tree differently; that is upstream's behaviour for every account.
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
| X4 | `calendar_event/set/za.rs:1182`: the `increment_sequence` call of the update deleted (base `de67af5df`) | `tests/src/za/jmap_calendar/oracle.rs:1801`, in `round_trip_keeps_what_an_ordinary_account_keeps`: the key account's lines hold no `SEQUENCE` line where the ordinary account's hold `SEQUENCE:1`; the oracle's main, quiet and inbound steps all passed first (M2 stored SEQUENCE 1 on both sides) | killed (ZA, exit 101, 369 s; no JK run needed). Recorded in `task-42-batch-E.md`; the ledger had no controller line on it when this note was written. The executor noted, without investigating, that `za_sequence_floor` probably raised SEQUENCE at the main steps. The X2 residual (the override branch inside upstream's function) is unchanged by it |

### Follow-up runs

- **J4s.** J4's edit with the corrected ZA-short recipe (the four `setup::` calls kept, `disabled::test` through `sharing_races::test` removed): `events.rs:1444`, "update_keeps_pairs_and_display_name: the update dropped the token pairs", the named catcher. The first ZA-short form removed `setup::test` too and stopped in `tokens::test`'s own setup (`tests/src/utils/za.rs:181`, 409 "not a key account"), so it did not reach the catcher.
- **Corrected first attempts.** J3's first edit queued in a batch with no document set and failed for that reason ("No document is set for a document task"); it was corrected once and run again. J8's and X3's first edits did not compile and were not run.
- **X1 after the strengthening**, above.
- No row was rerun for a result; every kill was observed once. The runner sets no `RUST_BACKTRACE`, so a panic inside a helper names the helper's line; the review checked each attribution by inference (J1, J2, J5, J9, J11, J16, J16b, J21) against the test source and each holds. Unit runs (J8, J9, J15, J16, J16b, J17, J26) have no command log (R42-12).

### Survivors

X1 survived and was answered with a strengthened test, which kills it. X2 survived and is an accepted residual in an upstream function. **Of the brief's 28 rows, no mutation survived.**

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

## Other harnesses

Task 41's records, all first runs, on the head of Task 40's first commit plus Task 41's two edits:

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
- **The three ordinary-account I/O costs** (spec 10): one cached account read at a code exchange, at a refresh and at a grant's deletion; one in-memory delete at a consent confirmation that carries no secret; one cached classification read in each calendar method. **D29's costs**: state-codec fields and context checks, with no global account lookup and no additional ordinary-account store I/O; key-calendar pages carry the origin fields and classify the account when choosing the continuation format. **The legacy grantee restart** (6.8): a grantee's legacy `r` continuation of a key calendar answers `cannotCalculateChanges`, and the client restarts from Initial.
- **4.3, a retried refresh.** An entry's expiry is computed when it is written, so on a retried refresh the next generation's entry can outlive its token by the retry's delay; the token's own expiry and the sweep bound it.
- **5.2.** Upstream stamps `updated` on an owner's update, so most JMAP moves ask `ModifyItems` as well; the two share levels cannot tell the difference.
- **Coverage that stays partial.** The refresh steps of 4.5 have a unit test for the classification only (`classify_refresh_cases`) and run otherwise in the za suite, where no case reaches the `Lost` or `Superseded` arm of the classifier (R18-4, N2). The pass-through of a foreign Bearer value (D26) has a unit test only.
- **R27-1, rollback.** Release 5's opener refuses a coloured personal bundle, so a binary rollback below release 6, or a cluster of mixed versions, leaves a coloured shared calendar unreadable until the colour is cleared or the newer binary returns.
- **X2.** The override branch of upstream's `increment_sequence` is covered by no test in this fork's net, for any account.
- **Inherited paged `/changes` after a log trim** (Task 22's concern 1). By reading `changes/get.rs` on its range-exhausted branch, the truncation check never sees a marker inside the range, so a record can be skipped; this affects ordinary `r` continuations too. It is upstream's behaviour, accepted as inherited.

## Deferred findings

What stays open after this plan, by task: every `minor (deferred ...)` line of the ledger, with its id where the ledger gives one. The arrow names where the ledger routed it. The review files (`task-<n>-review.md`, `task-<n>-fix-review.md`) hold each in full.

### Task 17

- (no id) A device-code poll whose `device_code` is the pending user code now answers `expired_token` for every account instead of `authorization_pending` (`token.rs:368-370`); no token was obtainable that way; not among the spec's every-account exceptions.
- (no id) The device-code guard uses `vault::tokens::CODE_LEN` where upstream generates with `DEVICE_CODE_LEN`; both are 40 and nothing ties them; fails closed if they diverge.
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
- R18-6 (plan-mandated): `Superseded` and `VaultChanged` refusals emit no fork trace; spec 8 says a refused token open is traced (→ a question for the spec).
- R18-7: a well-formed secret appended to a keyless grant's refresh token makes the precheck answer `Keyed`, and the `Lost` arm deletes that grant; the spec is silent.
- R18-8 (plan-mandated): an unusable token record makes each refresh delete its grant while D8's sweep leaves such a record alone (→ a mixed-version question for the spec).
- R18-9: parked refreshes hold a five-second client across Argon2 vault writes; passed under a one-minute load of 20 to 54.
- R18-10: the inherited linker warning and "tests (lib test) generated 1 warning" in the ZA and SYS logs.
- R18-11: `change_race_password` repeats `change_password`; `za_token.rs:123-126` repeats `:86-89`.
- N1: the comment at `crates/common/src/za/tokens.rs:410-411` should say "this request's rotation left the stored generation at `presented`, and it never falls".
- N2: no ZA case reaches `classify_refresh`'s `Superseded` arm after the fix; the vault unit test covers the classifier.
- O1: spec 4.5 step 2 and Task 18's plan steps did not state the R18-1 rule (reconciled later in `67917c44e`).

### Task 21

- R21-3: no test protects the grantee filter in `calendar/get.rs:108-130` or the try-next-calendar arm in `za_read.rs:81` (→ Task 36 or 42; still open after Task 36, see R36-4).
- R21-4: the R21-1 regression subscribes to `DataCorruption` only and does not show that no other event carries the canary.
- R21-5 (plan-mandated): one unreadable shared calendar fails a grantee's whole `Calendar/get` where the owner gets `notFound`.
- R21-6 (plan-mandated): two 20-line inline blocks in upstream's `calendar/get.rs`; the re-trace arm nearly duplicates `za_read.rs:70-79`; import groups reflowed.
- R21-7: the handlers classify the account again apart from the boundary's decision.
- R21-8: the view memo never hits in `CalendarEvent/get` and keeps every opened view alive to the method's end.
- R21-9: the event row read follows the content read; a concurrent update can give a spurious trace and a transient `notFound`.
- R21-10: `za_variants.rs`' module doc edited outside the brief's range; accurate.
- R21-12 (plan-mandated): the served-methods loop in `gating.rs` passes on `serverUnavailable`.

### Task 22

- R22-1: three account classifications per calendar `/changes` request.
- R22-2: the grantee sees one calendar, so its `Calendar/changes` paging asserts nothing about continuation pages.
- R22-3: non-canonical `z` strings are accepted, so `oldState` echoes a re-encoding.
- R22-4 (→ Task 43): upstream's `changes/get.rs` body moved unchanged into the `ChangesPage` trait; recorded as a merge hotspot.
- R22-5: the `invalidArguments` text is defined twice (jmap-proto `parser.rs`, jmap `za_calendar.rs`).
- R22-6: no test reaches the in-process `z` arms at `changes/get.rs`, `changes/za.rs`, `participant_identity/changes.rs`.
- R22-7 (→ Task 43 or a documentation commit): I §7.1's call-site list and the header's `za_variants` row no longer match the code (recorded under "Decisions and remaining qualification").
- R22-8: helpers not on the sheet; the fixture `jmapcal-sharing-plain-from` is outside the naming rule.

### Task 23

- R23-1: `let mut` edits an upstream line in `participant_identity/set.rs:52`.
- R23-2: the moved gating rows' comment cites spec 6.9 only; the header asks for 6.2.
- R23-4 (plan-mandated form): the `za_variants` served block passes on `serverFail` or `invalidArguments`.
- R23-5: the doc comment of `za_variants::calendars` was not updated for the served identities (superseded by Task 39's rewrite).
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
- R26-8 (carried to Tasks 30 and 31, closed there by the refusal of `hideAttendees: true`): nothing guards the reliance on no key event carrying `EVENT_HIDE_ATTENDEES`.
- N1: expanded parity compares starts only, and two fixtures share a start.
- N2: the expanded loop never asserts a non-empty answer.
- N3: the expanded form asks the flat ANDs only as one multi-property condition.
- O1: the rebuilt-count check does not attribute events to methods; the canary assertion's sensitivity on that path is unexercised.
- O2: the negated expanded form is checked for one negation only.

### Task 27

- R27-1 (→ Task 43): release 5's opener refuses a coloured personal bundle; recorded under accepted residuals and in README-dev.
- R27-2: no golden vector pins the bundle layout spec 11 depends on.
- R27-3: no unit test pins `open_shared` refusing a colour.
- R27-4: that rollback keeps the colour is shown by reading only.
- R27-5: the report names the wrong za module for the changed assertion.

### Task 28

- R28-3: `za_write_default` reads the previous default after `za_delete_calendar` may have cleared it; one edge differs from upstream, the effective default is the same.
- R28-4 (→ the whole-branch review): `za_commit_delete` returns the grantees' invalidation error after the deletion committed, so a committed destroy answers "incomplete"; inherited pipeline contract; reachability unverified.
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
- R29-7 (→ Task 43): the copied "Reminder"/SUMMARY description rule is a silent dependency; recorded in the merge hotspots.
- R29-8 (→ the owner): replacing alarms by value drops acknowledged or snoozed state that upstream keeps.
- R29-9: the brief adds a member fallback the spec lacks.
- R29-10: the SHIP, TM, FMT and mutation logs lack commands, source hashes and the mutation diffs.

### Task 30

- R30-1: no test of a store failure on a create; the existing `COMMIT_FAIL_ONCE` hook would drive one.
- R30-2 (plan-mandated): the disk-quota and UID checks run in another order than upstream's.
- R30-3 (plan-mandated): two classification reads per call for every account.
- R30-4: `create_two_objects_one_fails` cannot tell per-object commits from one batch.
- R30-5: no case checks the UID in a non-first calendar; several cases skip spec 9.2's CalDAV readback.
- R30-6: the unreachable missing-UID arm is a check of the fork's own.
- R30-7 (plan-mandated wording): the reworded comment at `set.rs:1890` is inaccurate for `CalendarEvent/copy`.
- R30-8: the store-failure trace says "write failed" for read failures too.
- R30-9: the interim arm answered an invalid update id `forbidden` (gone with Task 31).
- R30-10: `_can_delete_calendars` unused until Task 33; the head reads the account twice.
- R30-11: `ZA_INTERIM` sat in `set/za.rs` (removed by Task 33).
- R30-12: with duplicate `useDefaultAlerts` keys any true wins.

### Task 31

- R31-2: one occurrence's ruling refusal fails every sibling occurrence, including a destroy.
- R31-3 (plan-mandated): metadata on a synthetic id gets the fork refusal.
- R31-4 (plan-mandated): the quota check runs before scheduling, so a denied update answers `overQuota`.
- R31-5: the update path's D28 trace content is shown by reading only.
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

- R32-1: `placement_refused` asserts the error type only.
- R32-2 (→ Task 36): no test ran a placement change together with a fan-out (closed by Task 36's added case).
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

- R33-1: the D28 trace of a destroy's store failure is not asserted; the event writer's create and update traces are equally unasserted.
- R33-2 (plan-mandated): the own-items leg and `may_write_own` cannot be reached through the product and have no test.
- R33-3: `InstancePlan::DestroyEvent` is newly live and untested.
- R33-4: `no_itip_task_ever` still deletes over CalDAV.
- R33-5 (→ the whole-branch review): a destroy with scheduling of a future scheduled event whose sealed tree does not open is answered as the retryable D28 case and stops every later id; without the flag it succeeds.
- R33-6 (plan-mandated): a byte comparison cannot tell "not attempted" from "attempted and failed".
- R33-7: test hygiene (a helper defined twice, a repeated lookup, a keep-list use, a bare `created_id(0)` as an assertion).
- R33-8: the macOS linker note in every ZA and JK build.
- R33-9: `destroy` takes `&mut self` and mutates nothing; one name used three ways.

### Task 34

- R34-3: no case sends the triggering request with a Bearer token that carries a secret.
- R34-4 (→ the owner and Task 43): the hook runs before the `using` check, `allowed_capabilities` and every permission check, so a keyed token whose OAuth scope excludes calendars still drains when it names a calendar method (recorded under "Decisions and remaining qualification").
- R34-5 (→ Task 43's client notes): a `/set` with `ifInState` in a request whose own drain imported or merged an event answers `stateMismatch` once per arrival (recorded in `client-jmap.md`).
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
- R35-3 (→ Task 37, closed there): D13's "for every token, keyed or not" was asserted nowhere.
- R35-4: no failing run for `mayGetAvailability` left true.
- R35-5 (inherited, medium confidence; → the owner and Task 43): `Principal/getAvailability` for a key principal in an ordinary group with calendars returns the group's busy periods while `mayGetAvailability` is false (recorded under "Decisions and remaining qualification").
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
- R37-14: the doc comment's reason for the unreachable `za_is_impersonated()` clause is not the code's: a master-user login returns a fresh token with no session keys at all (`crates/common/src/auth/authentication.rs:415-431`), also when master and target are the same account.
- R37-15: the metadata read-back uses a get without the metadata capability.
- R37-16 (→ Task 43): the 404 detail pins upstream's text at `crates/jmap-proto/src/error/request.rs:238`; recorded as a silent dependency.
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
- R38-12 (→ Task 42): the kill run failed on `Expect`; no mutant showed a cross-side comparison failing (X3 later did).
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

- R40-7: the trace capture's only control is that it is not empty, and it ends before the revocation and the password change.
- R40-8: `handoff_exists` is asserted false after the exchange but never true before it.
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
- R41-2, R41-3 (→ Task 43 as two one-line corrections; not made, since this task edits documentation only, and left for the whole-branch fix wave): `tests/qualification/dispositions.json:1120` says `za_read.rs` serves "CalendarEvent/get, Calendar/get and Calendar/query" (its callers are `CalendarEvent/get`, `/query` and `/set`); `:1095` credits `common/src/za/tokens.rs` with the record's deletion by a vault write (that code is in `auth/vault.rs`).
- R41-4: the PRODUCT, fresh-store unit and FRESH receipts record no command, environment or HEAD.
- R41-5: the plan's step 1 script ignores dispositions and does not test equivalents or rename targets.
- R41-6, R41-7: the rerun decision rests on local evidence only; the report does not table the five runs.
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

- Task 22: `/queryChanges` origin forwarding has its runtime evidence in Task 26; the inherited paged `/changes` behaviour after a trim (above).
- Task 26: cost figures are local RocksDB, one node, debug build; that an external search store receives no term is by reading `dispatch/search.rs` only.
- Task 28: `za_delete_calendar`'s post-commit error reachability (R28-4) and `za_repair_owned_context` were not read.
- Task 31: after the fan-out, `za_commit_update`'s own error returns end the mail guard as `ZA_IMIP_ABANDONED` rather than `ZA_IMIP_DROPPED` (pipeline code); upstream's draft gate on scheduling and its hidden-participant path are not mirrored (they rely on no key event carrying those flags).
- Task 32: the placement UID check holds no lease and no batch fence (`pipeline/update.rs`), so a concurrent create or move of the same UID into the same calendar can both commit, while spec 3.3 says "checked under the UID lease" (true of creates only); on the plan retry `za_placement_contexts(..).await?` returns without `za_mail.dropped()` (the no-plan branch has the same shape).
- Task 33: a destroy in the same call as a failing create or update is shown by reading only.
- Task 36 (fix round 2's re-review): narrower mutants still survive by reading: a quiet link or unlink of an event with attendees, a quiet move between two unshared calendars of the same domain, a grantee's quiet move, recurring or multi-attendee events, the commit's bounded retry on a quiet move. X1's stated consequence (a later reply refused) is never observed, and the second quiet move has no kill evidence of its own.
- Task 38: R31-15 is not examined by the oracle, which has no grantee step; the oracle covers no sharing, alerts or queries.
- Task 40: no earlier scan meets a live token record, so the field-153 rule's only live exercise is `leak::test_jmap_calendars`; spec 9.5 words the iTIP-task rule as a scan rule "for every scanned account" while the code checks it in the module.
- Task 41: the CalDAV client was not run in either mode, so that leg of spec 5.1's proof of the move stays open until CI; the inventory and coverage gates run in Task 44's CI.
- Task 42: the unreached named catchers are not shown to fail under their mutants; a clean tree after each row rests on the hash check and the batch-end status; X2 ran under ZA only; the unscheduled JMAP update that omits `sequence` on an event with an override is compared by no test (X2's residual).
- R32-8 and R33-5 (above).

## Merge hotspots

`CLAUDE.md`'s paragraphs "Release 6 added merge hotspots" and "Release 6 silent dependencies" hold the full list: spec 11's two lists with Task 43's additions. In short:

- **Fork insertions in upstream files** (spec 11): `crates/http/src/auth/oauth/{auth,token}.rs`, `crates/common/src/auth/oauth/{token,grant}.rs`, `crates/http/src/auth/authenticate.rs`, `crates/common/src/auth/{vault,access_token}.rs`, `crates/types/src/field.rs`, `crates/services/src/task_manager/tasks/maintenance.rs`, `crates/jmap/src/api/{request,session}.rs`, `crates/jmap/src/principal/get.rs`, `crates/jmap/src/changes/{get,query}.rs`, the two `/set` handlers' head branches, the two `/get` handlers, two points in `calendar_event/query.rs`, `participant_identity/{set,changes}.rs`, the shrunken `dav` key branches, `crates/{dav,jmap}/Cargo.toml`, and D29's `crates/jmap-proto/src/{types/state,request/parser}.rs` and `changes/za.rs`.
- **Added by Task 43**: upstream's `changes/get.rs` body moved unchanged into `ChangesPage` (R22-4); the key writers repeat the heads of upstream's `/set` handlers; `za_itip_fails_write` names every `ItipError` variant; `za_split_bearer` relies on `TOKEN_HEADER`; D29's codec, context checks, origin preservation, legacy owner chain and `/queryChanges` forwarding; D11's recipient and description rules copied from `alerts.rs`; the query branch's reliance on no `EVENT_HIDE_ATTENDEES`; each use of `za_session_binding`; the pinned 404 text (R37-16); the oracle's dependence on upstream's converter; the calendar variants of `za_variants.rs` (R44); R27-1's rollback.

## Decisions and remaining qualification

**Approved.** D19 (decided 2026-11-14) and the explicit-origin D29 amendment (approved and recorded 2026-11-15) are approved; origin inference is not reopened.

**D29's evidence.** Task 22 (handler level, local RocksDB): `paged_sync_retention_collision` reproduces the retention-marker collision through the real HTTP handlers with a log truncated by the existing `truncate_change_log` API in setup, not by the maintenance timer: the first pages carry distinct origins, and after a rename the initial-origin page is served and the exact-origin page answers `cannotCalculateChanges`. `paged_sync_origin_survives_range_changes`, `paged_sync_legacy_and_ordinary` (a grantee's legacy `r` restarts from Initial; a key owner's `r` chain stays `r`; ordinary accounts' states byte-equal to upstream's), `calendar_state_slot_errors` and `precondition_matrix` (out-of-scope `z` is `invalidArguments` in a state slot and `stateMismatch` in a precondition slot) and the codec and parser unit tests (`za_calendar_state_*`, `za_calendar_state_contexts`). Task 26: `query_changes_paged_origin` pages `/queryChanges` from Initial and from an exact state, three pages or more, every intermediate state a `z` with its origin unchanged; exact-origin continuations are refused while a boundary is pending and after a rename, initial-origin ones served; legacy and ordinary forms as above. This is source and handler evidence; it is not the reference client's result. The checklist's D29 rows (`docs/zero-access/client-jmap.md`) are that separate gate, and they have not been run.

**New unresolved findings.** None was Critical or Important at its task's gate after the fix rounds. The open items below are each a ledger line.

**Observed status of the costs the brief names.** The memory of an unranged text query was not measured: the query holds every candidate's terms, and by R26-5 every opened event's full plaintext view, for the request (spec 6.4 accepted holding the terms). Its time is review focus 5's. The `/set` answer after D11 is upstream's, as spec 6.5 says: `events::create_use_default_alerts` asserts that a create sent with `useDefaultAlerts: true` answers with neither `alerts` nor `useDefaultAlerts`, and a create without the flag answers `useDefaultAlerts: false` (`events.rs:960`); the alerts appear on the next `CalendarEvent/get`.

**Open items for the project owner.** None blocked a task.

1. D15 as a sync-correctness rule, not an access control (ruling at Task 22): confirm.
2. R29-8: acknowledged or snoozed alarm state is lost with by-value default alerts.
3. Four questions from Tasks 17 and 18: the device-code poll with a pending user code now answers `expired_token` for every account (not among the spec's every-account exceptions); `Superseded` and `VaultChanged` refusals emit no trace against spec 8's wording (R18-6); a well-formed secret appended to a keyless grant's refresh token deletes that grant (R18-7); an unusable token record makes each refresh delete its grant while D8's sweep leaves such a record alone (R18-8).
4. The inherited paged `/changes` behaviour after a log trim (Task 22's concern 1), accepted as inherited.
5. R34-4: the drain runs before the `using`, scope and permission checks, so a keyed token whose OAuth scope excludes calendars still drains (spec section 2 says a scope limits what the server will do for the token).
6. R35-5: `Principal/getAvailability` for a key principal in an ordinary group returns the group's busy periods while `mayGetAvailability` is false.
7. The placement UID check holds no lease or batch fence while spec 3.3 says "checked under the UID lease" (true of creates only).
8. R36-10: an attendee that creates an event it does not organize, already answered, sends no reply from that create.
9. The two oracle rulings of Task 38: items and mail compared as the JSCalendar projection, and the script's explicit SEQUENCE with M7b leaving it to the server.
10. Task 39's F1 with R39-5: three consequences of D11 not in spec section 12. A constant description on every component would remove the first two and needs a change to D11's wording.
11. Task 39's F3 note: section 12 words the view-level consequence as "cannot rename or recolour"; the check also refuses `isVisible` and `isSubscribed`.
12. R41-8: the fresh-store restart script exercises nothing release 6 adds (no OAuth grant, no token record across the restart, no JMAP calendar call).
13. The independent CalDAV client was not run locally; that leg of spec 5.1's proof of the move stays open until CI.
14. R31-15: a grantee-organized event's JMAP update takes a different path from its CalDAV twin (origin tested against the owner's addresses, the floor and sender with the sender's roles).

**Proposed spec amendments, for the owner** (the spec file is not edited by this plan):

- **3.3**: add R36-10 (an attendee's JMAP create of an answered event sends no reply); qualify "checked under the UID lease" as true of creates, with the placement check against current archives without a lease.
- **3.3 and 3.4, 3.5's last sentence** (R38-11): record that a JMAP write stores upstream's converter form (`DURATION` for `DTEND`, the export's property order), stamps `CREATED` and the update stamps, and raises SEQUENCE for a client that omits it, so its iCalendar text differs from a CalDAV client's for the same edit; the oracle compares at the JSCalendar projection for that reason.
- **Section 12 (D11)** (R39-5): add the `iCalendar` member and override patches in subset reads, the stale description after a rename, and alarms that stay after a later `useDefaultAlerts: false`.
- **Section 12 (6.7)**: "cannot rename or recolour its own entry" becomes "cannot change its own entry, its visibility and subscription flags included".
- **6.2, 6.10, 6.11** (R35-5): "an empty list" holds only when the key principal is in no ordinary group with calendars.
- **Section 2** (R34-4): the drain runs for a keyed token whatever its scope.
- **10, every-account exceptions**: the device-code poll's `expired_token` answer for a pending user code.
- **8** (R18-6): whether `Superseded` and `VaultChanged` refusals are traced.
- **4.5** (R18-7, R18-8): a keyless grant presented with a well-formed secret, and an unusable record at a refresh.
- **9.5** (Task 40's note): the iTIP-task rule is checked in the module, not as a scan rule for every scanned account.

**Known staleness in the plan's own documents** (not corrected here; none of these files is in this task's list): R22-7 (the interface sheet's §7.1 call-site list and the header's `za_variants` row); R31-7 (the sheet's §7.2 says a view error fails the method; in `/set`, D28 makes it per-object); the plan header's routing lines; R41-2 and R41-3 in `dispositions.json`.

**Remaining qualification.** Task 44: the final matrix, CI on the branch (including the independent CalDAV client in both modes, the fresh-store job and the retention gates with the cohort's new figure) and the whole-branch review. Then, outside this plan: the reference client's checklist (`docs/zero-access/client-jmap.md`), the native-client rows of the manual checklist, other store backends, a second node, and the account page's text about how long keys stay in memory, which describes password clients only (spec 12, a follow-up outside this repository).
