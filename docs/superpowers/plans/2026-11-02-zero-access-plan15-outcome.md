# Zero-access invitations by email: plan 15 outcome

Written 2026-11-03 at the end of plan 15's implementation (`2026-11-02-zero-access-15-imip.md`, Tasks 1-12).
Plan 15 builds release 5 (spec `2026-11-02-zero-access-imip-design.md`, revision 3, approved at `1b1cd094d`): a key account receives an iMIP invitation, reply, cancellation or refresh as a sealed mail-borne item that its next session drains with upstream's merge; a key account's scheduling writes mail the recipients that are not key accounts (outside addresses, plain local accounts, local lists and groups) through the SMTP queue after the sender's commit, never through the task queue; an organizer's `SCHEDULE-STATUS` for a mailed recipient is `1.1`. A differential oracle compares a key account with a plain account step for step, and the leak, trace and mutation nets cover invariants 41-47. The whole-branch review runs after this note and fills its section.

## Where things stand

- Commits on `release5-imip`, cut from `release5-imip-spec` at `922b09449` (the plan, on top of the spec approved at `1b1cd094d`). The branch's commits carry the session's `Co-Authored-By` and `Claude-Session` trailers: the plan and Tasks 1-7 (with their fix rounds) name Claude Opus 5.5, the model of those dispatches; Tasks 9-11 and this task name Claude Sonnet 5.5 (ruling at Task 9: the trailer names the model that wrote the commit).
  - Task 1 `c9ac0673f` (upstream's `send_imip` composer and `itip_ingest` checks split into callable functions, no behaviour change);
  - Task 2 `10216a8e5` (the iMIP submission seam and the suite's SMTP sink), fix round `85fe660af`;
  - Task 3 `cd4e7474f` (the `X-ZA-SENDER` carrier and the item writer generalized over the sender);
  - Task 4 `fce064fea`, `f642c9a12` (a key account's inbound iMIP becomes a sealed mail-borne pending item), fix rounds `0d93d9bbb` and `190588982`;
  - Task 5 `8c59ae428` (the drain's mail arm);
  - Task 6 `310039546` (the mail recipient class, status `1.1`, the fan-out's mail attempts);
  - Task 7 `0b3f8afdb` (outbound iMIP submitted after the sender's commit, the commit-failure hooks), fix round `db5bfbb0e` (the abandonment guard);
  - Task 8: no commit (the other harnesses' audit and runs);
  - Task 9 `c8456daa7` (the differential oracle), fix round `66c1bfe78`;
  - Task 10 `4d0e20884` (the iMIP trace module), fix round `301c046f8`;
  - Task 11 `f13e0075a` (the leak scan of mail-borne items, outbound mail and the task queue);
  - Task 12: the commit of this note with the release 5 checklist section and `CLAUDE.md` (docs only; the mutation runs applied temporary edits and reverted them, no product code changed).
- The verification matrix of Task 12 Step 4, run on the branch head `f13e0075a` (the tree of this commit differs from it by the three documents only), one cargo command at a time, long runs in the background:

  Every command ran on its own on the head's tree, in this order, with the shipping features or the test features CI uses (`STORE=RocksDb RUST_MIN_STACK=16777216` for the integration suites). Times are the suite's own `finished in` (integration) or the command's wall time (the rest, which include any rebuild). Every suite passed on its first run; no known flake occurred, so no rerun was needed.

  | Check | Command | Result | Time |
  |-------|---------|--------|------|
  | formatting | `cargo fmt -p services -p groupware -p email -p dav -p common -p store -p tests -- --check`; `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` | no difference | 2.5 s |
  | compile check, shipping | `cargo check -p stalwart --no-default-features --features rocks`, then the globals' filter | exit 0, 0 warnings in the log, filter `clean` | 15 s |
  | compile check, tests | `cargo check -p tests --tests` | exit 0, 0 warnings (none in `tests/src/za/imip/` or `smtp_sink.rs`) | 17 s |
  | vault and groupware units | `cargo test -p vault -p groupware --features vault/test_mode` | groupware 188 passed, vault 46 passed | 50 s |
  | common | `cargo test -p common za::` | 14 passed | 7 s |
  | email | `cargo test -p email --features test_mode za::` | 10 passed | 25 s |
  | store | `cargo test -p store --lib --features test_mode za_test` | 8 passed | 0.4 s |
  | services | `cargo test -p services --lib za_imip` | 3 passed (no features needed) | 34 s |
  | smtp units | `cargo test -p smtp --lib --bins --features test_mode` | 16 passed | 49 s |
  | http units | `cargo test -p http@1.0.1 --lib --bins --features test_mode,enterprise` | 6 passed | 65 s |
  | leak scanner unit | `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact` | 1 passed | 49 s |
  | za suite | `cargo test -p tests za::za_tests -- --nocapture` | 1 passed; `Mail leak scan: 18148 records, 142 archives, 59 blobs, 59 search index records (2 of the account), 2 emails, 2 stubs, 4 visible hits` | 486.58 s |
  | za suite, filesystem blob store | the same with `BLOB_STORE=FileSystem` | 1 passed | 440.64 s |
  | cache swap | `cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture` (alone, no `BLOB_STORE`) | 1 passed | 19.56 s |
  | CalDAV, plain | `cargo test -p tests webdav::webdav_tests -- --nocapture` | 1 passed (`cal_itip` passed first time) | 19.41 s |
  | CalDAV, key accounts | the same with `ZA_KEY_ACCOUNTS=1` | 1 passed | 21.85 s |
  | IMAP, plain | `cargo test -p tests imap::imap_tests -- --nocapture` | 1 passed | 10.27 s |
  | JMAP, plain | `cargo test -p tests jmap::jmap_tests -- --nocapture` | 1 passed (`calendar::notification` passed first time) | 128.95 s |
  | IMAP, key accounts | the same with `ZA_KEY_ACCOUNTS=1` | 1 passed | 12.24 s |
  | JMAP, key accounts | the same with `ZA_KEY_ACCOUNTS=1` | 1 passed | 91.06 s |
  | SMTP | `cargo test -p tests smtp -- --nocapture` with CI's `SPAM_RULES_URL` | 76 passed, 3 ignored, 0 failed (none of the six timing-flaky tests failed) | 46.27 s |
  | CalDAV query expansion | `cargo test -p tests 'webdav::cal_query::' -- --nocapture` | 2 passed, 1 ignored (as CI's note) | 0.01 s |
  | store tests | `store::store_tests` | 1 passed | 6.08 s |
  | blob tests | `store::blob::blob_tests` | 1 passed | 4.25 s |
  | lookup tests | `store::lookup::lookup_tests` | 1 passed | 12.12 s |
  | search tests | `store::search_tests` | 1 passed | 256.41 s |
  | telemetry tests | `telemetry::telemetry_tests` | 1 passed | 2.23 s |
  | system tests | `system::system_tests` | 1 passed | 60.92 s |
  | CI step table | `python3 -I scripts/tests/ci_steps.py` | 0 differences (no gated step changed) | 0.1 s |
  | gate tests | `python3 -I scripts/tests/test_gates.py` | 43 tests OK, 2 skipped | 4.2 s |
  | product build | `cargo build --release --locked -p stalwart --no-default-features --features rocks` (the shipping command with CI's `--locked`; fat LTO, no other cargo command alongside) | exit 0, `Finished release profile in 20m 15s`; the log holds no warning at all, so none names a fork file | 20 m 15 s (1216 s wall) |
  | product identity | `target/release/stalwart --version` | `Stalwart Server fork v0.2.0-beta.1 (upstream v1.0.1 23b69755cb3cd8ecd952fe0443c5ac067a506184; source f13e0075a46d552e00c2cf5a2f13351bdf65e77a; dirty)`: `dirty` because `CLAUDE.md` and the checklist were edited and uncommitted during the build; the committed tree reads `clean` | 0 s |
  | fresh-store failure tests | `python3 -B -m unittest discover -s tests/product -p test_fresh_store.py -v` | 9 tests OK | 6.7 s |
  | fresh-store gate | `python3 -B tests/product/fresh_store.py --binary target/release/stalwart --log-dir <workspace>/t12-fresh-store-logs` | exit 0: `work drained: SMTP queue empty, index queues empty, no task due; scheduled for later: ShareRevokeCleanup (Pending)`, then `PASSED: 9 readback checks after the restart` | 9.5 s |

  - `automation::automation_tests` and `directory` need Docker, which is not installed on the machine (`docker` is not found): **left to CI**. The CalDAV client (`caldav-client` job) is **pending for CI** too (no local Python has its packages, nothing installed, ruling P5). `cluster_tests`, `stress_tests`, `scim_tests` and the external protocol suites are not run by any job, as `CLAUDE.md` records.

- What now exists:
  - the submission seam `common::za::imip` (`ZaImipMail`, `za_imip_submit`, `za_imip_submit_all` with its abandonment guard) and the submitter `services::za_imip`, a local SMTP session on upstream's extracted composer (`ImipContext`, `ImipComposer`, `compose_imip_message`): quiet sender lookup, no RSVP token or block, `za_key_rcpt` set, a fresh span id per submission, every outcome traced with ids, the SMTP stage and the reply code only (spec 5.3, D11, D12, section 7);
  - `za_imip_item` and `za_imip_uid_known` (`crates/email/src/message/za/imip.rs`): delivery's ordered checks (size, parse and Exchange fill, snapshot and the method's sender rule, method, key-account sender refusal, known UID with the auto-add predicate for a REQUEST) and the sealed mail-borne pending item, whose sender rides only in the sealed `X-ZA-SENDER` carrier and whose row names `CalendarAddress("")` (spec 4.1, 4.2, D1-D4, D16);
  - the drain's mail arm (`apply_mail`, `delete_mail`, `answer_refresh`): the checks repeated on the opened tree, upstream's import or merge with the carrier's sender and no token, the REFRESH answer by mail after the item's deletion commits, the organizer-copy gate of the delegating fan-out, internal delegates only (spec 4.3-4.5, D5-D7, D14, D15);
  - the mail recipient class (`Recipient::Mail`, `Route::Mail`), `ITIP_STATUS_SENT = "1.1"` at rank 0, `internal_only` on `za_fan_out` and `ZaFanOutResult.mail`, submitted by the DAV PUT (create and update) and DELETE after their commits return `Ok` (spec 5.1-5.5, D9, D10, decisions 9 and 10);
  - the hooks `IMIP_SUBMIT_FAIL_ONCE`, `UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_FAIL_ONCE` and `SECOND_CHUNK_FAIL_ONCE`, the suite's SMTP sink (port 12525, the `za-sink` route for every non-local domain), `wait_for_queue`, `clear_received_scheduling` and `expire_and_purge_blobs`;
  - the za modules `imip::{inbound, outbound, oracle, traces}` after `pinning::test` and `leak::test_imip` after `leak::test_mail`, with the scanner's rules for `X-ZA-SENDER`, empty-address rows, the one-unit padding and the held-consumer task queue;
  - the release 5 section of the manual checklist, item 13's last sentence in its release 5 form, and `CLAUDE.md`'s release 5 entries (code locations, invariant 41-47 bullet, hooks and helpers, fixtures, merge hotspots; no status paragraph, the controller adds it at merge time).

## What changed and why

**Task 1, the extractions (spec 3.1, 8.1; spec questions 4 and 5).** `send_imip` keeps its loop and its three events; its setup, per-message step and per-recipient step became `ImipContext`/`imip_context`, `ImipContext::composer` and `ImipComposer::submit` (parameters `ImipSubmitOptions { rsvp, za_redact, session_id }`, a `SenderLookup` of `Traced` or `Quiet`), with `compose_imip_message` and `ItipTemplate::from_template` split out so the composer is testable without a `Server`. `itip_ingest`'s checks became `itip_parse_inbound`, `itip_fill_organizer`, `itip_sender_snapshot` and `itip_auto_add_allowed`, called by upstream's code with upstream's arguments. A unit test holds a verbatim copy of v1.0.1's message builder as the byte oracle (ruling P2). No ordinary account's behaviour changed.

**Task 2, the seam and the sink (spec 5.3, D11, D12, section 7, 8.2).** `common::za::imip` holds the function-pointer seam (`services` registers the submitter from `spawn_services`; code, not state), one (message, recipient) per call, never in the task queue or any store location. The submitter opens upstream's local session with `rsvp: false`, the quiet sender lookup and `za_key_rcpt` set, submits MAIL, RCPT and the queue write, and maps the outcome to `Queued`, `Refused { stage, code }` or `Failed`. The suite gains an SMTP sink (`smtp_sink.rs`) and a `za-sink` relay route for every non-local domain. The fix round made the seam case fail if the sender lookup becomes traced or an event names the owner.

**Task 3, the carrier (spec 4.2, D4).** `X-ZA-SENDER` joins `X-ZA-RSVP-TOKEN` and `X-ZA-EXTRA` as a pending item's third root carrier: the sender sealed with the item's DEK, associated data `za/v1|itip-sender|<recipient id>|<UID>`, root order `X-ZA-SEALED`, at most one carrier, `X-ZA-KEY`. `write_item_inner` became `za_write_pending_item`, generalized over `changed_by` and the sender. The drained form carries none of the three.

**Task 4, delivery (spec 4.1, 4.2, 4.5, 4.6, D1-D3, D16, section 7).** The iMIP gate of `email_ingest_into` loses `!za_key`; a key account's part goes to `za_imip_item`, which writes at most one sealed pending item under the served public key and never reads a session key. A refusal writes nothing, is traced with ids and a fixed string, and never fails the delivery. The known-UID rule counts an event or a pending item (the UID is visible on a sealed item). `fanout::test_plain_owner`, `fanout::test_task_queue`, `gating::test_scheduling` and `mail::fence` were adjusted for the items they now receive.

**Task 5, the drain's mail arm (spec 4.3-4.5, D5-D7, D14, D15's gate).** An item that opens with `X-ZA-SENDER` goes to `apply_mail`: the Exchange fill, the snapshot and sender rule, the key-account sender refusal and the auto-add predicate are repeated, then upstream's import (a REQUEST with no event) or merge (an event exists) runs through the drain's commits with `Incoming::Mail`, no token, no stale-reply check and no pair change. `Method::Add` and `Method::Refresh` are accepted from mail only. Every outcome upstream's ingest leaves no inbox item for deletes the item (`delete_mail`). A REFRESH is answered by mail only after its item's deletion commits. A delegating REPLY into the account's own organizer copy fans out to new internal delegates.

**Task 6, outbound routing (spec 5.1, 5.2, 5.4's status, D9, D10, decisions 10).** `resolve_inner` returns `Recipient::Mail` for no local account or a local account that is not a key account; `route` sends a mail recipient every message upstream's builders address to it. `za_fan_out` takes `internal_only` (D15), writes `1.1` before the commit for each mailed recipient (`5.3` under `internal_only`) and returns the mail attempts. A key account without `CalendarSchedulingReceive` or a served key, the sender's own account and the delegated organizer path's owner stay `NotInternal`.

**Task 7, submission after the commit (spec 5.4, 5.5, 5.6, decision 9, 8.2, 8.5).** The DAV create, update and DELETE submit `ZaFanOutResult.mail` only after their commit returns `Ok`, outside the create's UID lease; a failed DELETE commit traces `ZA_IMIP_DROPPED` and mails nothing. The hooks `UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_FAIL_ONCE` and `SECOND_CHUNK_FAIL_ONCE` (a test-mode line in `Store::write_batch`) drive the commit-failure cases; `imip::outbound` covers them with the mailed-once, plain-attendee, list, alias, delegation, tracing and large-invitation cases. The fix round added the abandonment guard in `za_imip_submit_all` (a dropped request future traces `ZA_IMIP_ABANDONED` for the attempts it leaves unsubmitted) and kept the DELETE's invalidation result so its error cannot lose the CANCELs.

**Task 8, the other harnesses.** An audit and a run of key-mode `webdav_tests`, `imap_tests` and `jmap_tests`, the independent CalDAV client and the fresh-store gate; none routes mail to the sink and none changed (records below).

**Task 9, the differential oracle (spec 3.3, 8.3).** `imip::oracle` runs the same four inbound and four outbound steps against a key account and a plain account and compares the stored event, the inbox items, the sink's messages (envelope, headers, parts) and the statuses under the normalizations of 3.3 (DTSTAMP, the address mapping, header folding, `SCHEDULE-STATUS` in stored copies and CANCELs only). A self-check proves each rule absorbs only what 3.3 allows.

**Task 10, the trace module (section 7, D17, invariant 47).** `imip::traces` captures every event type through the delivery, drain, PUT and DELETE paths and asserts that every iMIP event carries ids, counts, stage names, reply codes and fixed strings only, that `From` is `[redacted]`, and that the fork's own sender lookups stay quiet at delivery and in the drain (the one exemption is upstream's DKIM signing-domain lookup, once per message).

**Task 11, the leak module (spec 8.4; 4.2, D4; invariants 41, 43, 47).** `leak::test_imip` scans the whole store for the module's canaries after inbound mail, outbound mail, a REFRESH answer and with the task consumer held. The scanner knows `X-ZA-SENDER` as a pending item's only carrier, refuses an address in a row and a pending mail-borne item without a carrier, and checks the carrier's one-unit padding.

**Task 12, this note (spec 8.6, 8.7, 9, 10).** The mutation sample, the release 5 section of the manual checklist, `CLAUDE.md`, the verification matrix and this outcome note.

## Spec questions

The plan's seven questions (Jay approved all seven as written, "yes. approved. subagent execution please."), with what execution found:

1. **`fanout::test_plain_owner` (`tests/src/za/fanout.rs`) missing from 8.5's inventory.** Confirmed. key1's invitation into a plain account's shared calendar is mailed by upstream's task to key2, and from Task 4 on that mail and the DELETE's CANCEL are two mail-borne pending items in key2's inbox. Task 4 asserts them (a REQUEST, then a CANCEL known by the pending REQUEST, D3) and deletes them.
2. **The drain's repeat of the Exchange fill has no recipient address.** The drain fills with the account's first address (`AccountInfo::addresses().first()`; no fill when the list is empty), in `drain_item` before the first snapshot as upstream's parse fills before its snapshot. On every item delivery wrote, the repeat is a no-op.
3. **The two-session REFRESH case names no hook.** The mail arm calls the existing `drain_lookup_pause_point` after its UID lookup and before `merge`; `import` keeps its own call. `refresh_two_sessions` holds one session there and asserts one answer at the sink.
4. **"The submission's SMTP span id".** Upstream's local session passes span id 0. The per-recipient function takes the session id; `send_imip` passes 0, and the fork passes a fresh id from its dummy instance's `span_id_gen` (independent of the listeners' generator, so a same-millisecond collision is possible, as upstream's own property).
5. **3.1 item 1's "two functions".** Built as a context type with the task-level setup and the per-message step, plus the per-recipient function, with `compose_imip_message` and `ItipTemplate::from_template` split out. No behaviour change (Task 1).
6. **8.2's REQUEST merging into a copy that holds pairs.** An outside party's REQUEST for a key-organized UID, delivered to the key attendee, is rejected by the merge (`OrganizerMismatch`), the item deleted and the pair unchanged. The group-organizer path of 5.1, the one route to a successful merge, is not exercised.
7. **8.5 misses `leak.rs`'s `SCHED_EVENT`.** key2's invitation of `plain@example.com` is mailed from Task 7 on, so the canary would reach plain's mailbox, calendar and index in the clear; that ATTENDEE is marked `SCHEDULE-AGENT=CLIENT`, so upstream's builder makes no message for it and the event still tests the organizer's sealed copy. The suite-wide `plain@example.com` is cleared of what it receives in `fanout::test` and `gating::test_scheduling` (`clear_received_scheduling`).

## Rulings

Every ruling of the plan's ledger (`.superpowers/sdd/2026-11-02-zero-access-15-imip/progress.md`, git-ignored), verbatim and in ledger order: the pre-flight rulings P1-P6, then each task's. The ledger's pre-flight table rows that only cite a ruling (P2-P5) are not repeated. The line "Ruling: every later dispatch carries the inventory-gate rule" belongs to no task.

- Ruling P1: fix rounds 4-5 dispatch a fresh implementer at opus, not one tier above — Jay's tier cap forbids a subagent above the session's model (Opus 5.5) without his authorization — costs a smaller capability step at round 4 if a loop sticks.
- Ruling P2: Task 1's verbatim copy of v1.0.1's MessageBuilder expression inside the test stays — it is spec 8.1's byte oracle for the extraction, test code, not product logic — if wrong, one duplicated block in a test module.
- Ruling P3: Task 5's drain cases run inside imip::inbound::test after Task 4's delivery cases and before its cleanup, draining the items Task 4 left (imip-in-1, imip-in-d3) — the plan's case texts presuppose them ("the Task 4 REQUEST imports", "Task 4's items") — if wrong, some case reorder in Task 5.
- Ruling P4: Task 7's mailed_once holds the task consumer before the statuses PUT (the PUT runs under the hold; it is not repeated) — "no task (the consumer held)" is only checkable that way — if wrong, a restructured case.
- Ruling P5: Task 8 Step 4 uses an existing local Python environment for the CalDAV client if one has the packages; otherwise it installs nothing, records the run as pending for CI's caldav-client job, and the controller raises it with Jay at the finish — the plan says ask before installing and a mid-run stop buys nothing — if wrong, the key-mode CalDAV client result comes from CI only.
- Ruling P6: every review and scoped re-review runs at opus, read-only, no cargo (reviewers name the test they would run) — Jay's rules: reviewers take the ceiling; reviews after a verified round are read-only — costs more tokens than mid-tier re-reviews.
- Task 1: Ruling: finding 1 (a_template_without_rsvp_renders_no_rsvp_block cannot catch a dropped `options.rsvp &&` guard before set_recipient; "mints no token" unasserted) — no Task 1 fix round: the guard is only reachable through `submit`, which needs a Server; Task 2's seam test (no "/calendar/rsvp" in the html) is the first check that can fail on it, provided RSVP links are enabled on the za suite's server, and mutation I10 (Task 12, `rsvp: true`) has the same observable effect as dropping the guard; carried into Task 2's dispatch (confirm RSVP enabled so the absence check can fail) — if wrong, a dropped guard goes unseen until Task 7's mailed_once or Task 12's I10.
- Task 2: Ruling: F1 (nothing fails if services::za_imip's SenderLookup::Quiet becomes Traced or za_redact: true becomes false; invariant 47 is Task 2's in the case map) — fix round: the seam case gains a trace collector asserting no event outside Smtp/Queue/Delivery spans names the owner's address and the seam's own events carry no '@', kill-checked with Quiet→Traced; for the flag, the implementer establishes whether za_key_rcpt has any observable effect on a local Session::local + queue_message(Internal) transaction under the suite's configuration: if so, an observation plus kill check; if not, an evidenced equivalent-mutant record for the outcome note's residuals (D12 then holds as defence in depth) — if wrong, either a test that tests nothing or a silent regression of the redaction flag.
- Task 2: Ruling: F4 (outbound events carry no From key) — spec 7's table lists `From [redacted]` on the delivery rows only and the brief follows the table; the "From stays as a key" sentence is read with those rows — if wrong, dashboards grouping outbound ItipMessage* events by From see no key for key accounts (one key to add).
- Task 3: Ruling: F1 (spec 4.2 "exactly one of X-ZA-RSVP-TOKEN, X-ZA-EXTRA or X-ZA-SENDER" vs the code's "at most one") — the code stands; release 2 fan-out REQUEST/CANCEL items legitimately carry no carrier (route's pair: union.only(recipient) is None for a non-organizer writer's copy or a removed attendee with no stored pair, fanout.rs:1193-1199, :364-368, :1265-1277), and the drain handles them (drain.rs:1304); record a spec erratum in the outcome note (Task 12): "at most one; a mail-borne item carries exactly X-ZA-SENDER" — if wrong, nothing in code changes; the spec text misleads a reader.
- Task 4: Ruling: finding 1 (fence.rs:768 assert_no_calendar_data races the queue's 1 s local retry: assert_first_attempt_lost may return before the retry, Traced::finish sleeps 500 ms, and the retry now legitimately writes one pk-sealed pending item) — fix round: the stale check holds whether or not the retry ran (no CalendarEvent document; every CalendarEventNotification present is pending with envelope_wrap == Some(WRAP_PK)) — if wrong, a rare CI flake in mail::fence.
- Task 4: Ruling: the new Important — fix round 2: restore the name assert_no_calendar_data with the new body (the finding never required a rename; smallest change, no qualification-file edit) — if wrong, the weekly gates job goes red on one helper.
- Ruling: every later dispatch carries the inventory-gate rule (renaming or removing a tests/src function listed in tests/qualification/baseline-0.16.27.json needs a renames.functions entry or a disposition; prefer keeping the name) — added to implementer-contract.md's Code rules — the branch's only removed tests/src function so far is assert_no_calendar_data (git diff 922b094..HEAD) — if wrong, the weekly gates job goes red after merge.
- Task 4: Ruling: round 2's re-review done inline by the controller instead of a reviewer dispatch — the diff is two lines (definition and call site renamed back; git grep finds no other reference) — if wrong, nothing: a rename revert has no other surface.
- Task 5: Ruling: F1 (delete_mail traces ZA_IMIP_ITEM_DELETED before delete() commits, as the brief orders; the losing REFRESH session and a store-error retry each trace a deletion that did not happen) — kept for now, carried to Task 10 (count ItipMessageSent / sink answers, never deletion traces) and to the final review as a candidate for the fix wave (trace after Ok(true)) — if wrong, dashboards see duplicate deletion traces under a two-session race.
- Task 6: Ruling: concern 1 (a writer-only plain group addressed by the delegated second run is mailed, 1.1) — accepted: spec 5.5's "second-run self items stay internal" covers items to the writer's own account (a key account, never the mail class), and the sharing spec rev 14 ("What is kept from the second run") makes L's writer-only group addresses "normal delivery targets ... the ordinary status when attempted", which at release 5 is mail (decision 10); the reviewer checks that no address can be mailed by both runs of one write — if wrong, one condition in phase 1 (mail only for Upstream provenance) and the assertion back to 5.3.
- Task 6: Ruling: F1 (a Failed lookup on a route release 2 does not attempt records 5.3, not spec 5.1's 5.1: fanout.rs:512-519, :699; e.g. a key attendee's REPLY to an outside ORGANIZER under a transient store error) — not a fix round now; carried to the final fix wave (record ITIP_STATUS_DELIVERY_FAILED whenever recipient == Failed, whatever the route; nothing is mailed either way) — if wrong, until then a transient lookup failure reads "not attempted" instead of "failed" for that address.
- Task 7: Ruling (provisional, for the reviewer to confirm): the tracing case's post-commit allowance of span-less Cache events from upstream's RCPT lookups matches spec 7 ("the SMTP session and queue that carry a mailed invitation trace its envelope as they trace every message (transit metadata); the fork's own lookups stay quiet") — the pre-commit strict window still catches a traced fork lookup in the fan-out, and Task 2's seam case catches a traced composer lookup — if wrong, a fork lookup emitting traced Cache events after the commit goes unseen by tracing::test.
- Task 7: Ruling: I1 (mail is submitted inside the request future; hyper drops it when the client disconnects between the commit and the end of za_imip_submit_all, so the remaining submissions are cancelled with no trace — section 7 "a submission that fails after the commit is traced"; a DELETE's CANCEL is then lost permanently) — fix round: a drop guard over the unsubmitted remainder (the in-flight one included) that traces one Calendar(ItipMessageError) per distinct document id with a new fixed constant, disarmed when the loop completes; recorded as a residual beside the crash in the outcome note (the mail is still lost, now visibly); detaching the loop (tokio::spawn) is NOT taken — it rewords invariant 41 and Jay's "composed in the user's request" decision — raised with Jay at the finish as the alternative — if wrong, a disconnect still loses mail, but it is traced either way.
- Task 7: Ruling: M2 (delete.rs:592 invalidate_caches(..)? returns Err after the commit and before the submission, losing the CANCELs untraced; plan-mandated placement) — raised into this fix round despite its Minor label: it is the same defect class as I1 (an untraced mail loss after a successful commit, section 7) and a reorder in the same lines — submit before the invalidation, or hold its result, submit, then return it — if wrong, one reordered call.
- Task 8: Ruling: brief Steps 3-4 'ask Jay before installing' / 'stop for Jay' replaced by P5 — install nothing, record the CalDAV client as pending for CI if packages are missing, record any Python-harness queued mail and continue; the controller raises both at the finish — if wrong, Jay sees the finding at the finish rather than mid-run.
- Task 8: Ruling: the product build uses CLAUDE.md's thin-LTO local form (CARGO_PROFILE_RELEASE_LTO=thin, CODEGEN_UNITS=16) — a local test binary for fresh_store.py and the CalDAV client, not a release artifact — if wrong, the fat-LTO product build is left to product-build.yml.
- Task 8: Ruling: Task 8's review done inline by the controller (no diff to review; the four records carry file:line evidence; spot-checked: route_outside_mail's only caller is za/mod.rs:129, za_variants.rs:423 invites the key account jane only, cleanup.rs:75 makes QueueMessage a leftover, fresh_store.py has no ATTENDEE/ORGANIZER) — if wrong, an outside attendee in a key-mode harness would show as a QueueMessage leftover or a CI failure.
- Task 8: Ruling: the old venv in the main checkout's 1.0-upgrade workspace (.superpowers/sdd/2026-10-27-stalwart-1-upgrade/task-8b/venv) is not used — it is another plan's workspace (the skill's rule) and outside this worktree; raised with Jay at the finish beside P5's pending record — if wrong, the CalDAV client's first release 5 run is CI's.
- Task 9: Ruling: c8456daa7's trailer "Co-Authored-By: Claude Sonnet 5.5" stays — the commit was written by a Sonnet subagent, so it is the accurate attribution; no history rewrite — if wrong, one trailer to amend before the PR.
- Task 9: Ruling: F1 (normalize strips SCHEDULE-STATUS from every iCalendar value — item trees and every sink message's ICS — where 3.3 item 4 allows it only in stored copies and CANCELs; a fork-only SCHEDULE-STATUS=1.1 in an outbound REQUEST/REPLY would pass) — fix round: apply the rule to the stored event and to sink messages whose method is CANCEL only (t9-dev2.log shows the real exchange needs no more) — if wrong, the stopping criterion stays blind to a fork status leaking into outside mail.
- Task 9: Ruling: F3 (the brief's Step 3 combined negative case — DTSTAMP, a mapped address, SCHEDULE-STATUS and SUMMARY all differing must compare unequal — is missing) and F4 (deliver then trigger_drain with no wait_for_queue at inbound steps 1, 2, 4 and outbound step 2; a late local delivery leaves the item pending and fails "drained" — a flake, as Task 4's fence race was) — raised into this fix round despite their Minor labels: F3 is a brief-mandated case, F4 a flaky za suite case with a one-line fix each — if wrong, two extra lines.
- Task 10: Ruling: the recipient span rule follows Task 7's adopted rule (strict before the parked commit; after it, Smtp/Delivery/Queue events, their spans and Cache events allowed; never an ItipMessage*, fan-out or drain event) — the brief's literal span rule cannot hold at HEAD (upstream's rcpt_resolve Cache events carry no SpanId) and spec 7 accepts the SMTP session's envelope tracing as transit — if wrong, a post-commit recipient-keyed fork lookup goes unseen here too (Task 7's recorded blind spot).
- Task 10: Ruling: F1 (the module cannot fail if delivery's step-4 sender lookup (email/src/message/za/imip.rs:244) or the drain's (drain.rs:840) stops being quiet: remote senders are looked up by domain only (Key "remote.test"), and the key sender key1's traced Key "key1" is neither a canary nor in `ours`; spec 7 "the fork's own lookups stay quiet", invariant 47; the case map gives both to Task 10) — fix round: the module must fail, kill-checked, when either call site is swapped for the traced account_id_from_email; fixture/assertion is the implementer's (a canary domain label on a remote sender, or no non-transit Cache event with Key "key1" in the delivery window), with any exemption for upstream's own lookups limited to the exact (event type, key) seen in a control run and justified as SMTP-session transit; plus F6's positive control (assert non-transit Cache events do reach the collector) so the check cannot pass vacuously — if wrong, invariant 47 stays unguarded on the inbound path.
- Task 10: Ruling: F3 (outside_transit exempts every event in a transit span, ItipMessage* included — ruling 1's never-exempt clause not implemented; the LMTP session's MAIL FROM names the sender canary, so the sender check is blind to the whole delivery path) and F2 (the Cache exemption applies in every Open window, not only post-commit as in tracing.rs; needed strict for F1's drain check) — into this fix round: F3 is a dispatch ruling not implemented, F2 a precondition of F1's fix — if wrong, two narrowed filters.

## Mutation runs (plan 15)

### Task 12, Step 1 (spec 8.6)

Twenty-two rows (spec 8.6's table, with the plan's I1-I19; I13 has four sub-rows), each applied to the committed head `f13e0075a` by a script of exact string replacements (each required to match exactly once), one at a time. For each row the za suite ran (`STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`, which stops at its first panic), the edited files were restored with `git checkout -- <file>` and `git status --short crates/ tests/` was empty before the next row. None was committed. Logs and exact diffs are `t12-mut-<id>.log` and `t12-mut-<id>.diff` in the git-ignored workspace. A run took 45-165 s including the rebuild, the suite 18-111 s to its first failure.

**22 of 22 rows were killed; no mutation survived.** Where the first failure is not the named catcher, an earlier module of the suite (an assertion adjusted in Tasks 4, 6 and 7 for the new behaviour) fails first, so the table gives the first failure and the follow-up runs below isolate the named catcher for the rows whose own case it was worth confirming. Several rows describe code by intent, and HEAD's shape differs: the edit column is what was actually changed.

| Id | Row of 8.6 | Exact edit | Named catcher | First failure observed |
|----|-----------|------------|---------------|------------------------|
| I1 | `!za_key` restored | `ingest.rs`, the iMIP gate: `&& !za_key` added after `&& !is_spam` | `imip::inbound::delivered_request`: no item | `tests/src/za/fanout.rs:1272`, "one mail-borne item in key2's inbox", left `0`, right `1` (Task 4's adjusted assertion in `fanout::test_task_queue`, 18 s in; `imip::inbound` not reached; Task 4's RED 2 is this state) |
| I2 | item without `X-ZA-SENDER` | `za_imip_item` step 6: the writer's sender argument `Some(&sender)` became `None` | `delivered_request`: no carrier; `replies` | `fanout.rs:1283`, left `None`, right `Some("plain@example.com")` (the carrier's sender in `test_task_queue`) |
| I3 | sender in `changed_by` | `ChangedBy::CalendarAddress(String::new())` became `ChangedBy::CalendarAddress(sender.clone())` | `leak::test_imip`: visible rows | `fanout.rs:1272`, as I1 (the helper selects mail-borne items by their empty-address row: left `0`, right `1`); `leak.rs:2837` is reached only in Task 11's reduced run |
| I4 | no key-account sender refusal | `za_imip_item` step 4: the `Ok(true)` arm (`refused(ZA_IMIP_KEY_SENDER); return;`) became `Ok(true) => {}`; the lookup stays | `refused_deliveries`: the key account's mail yields an item | `imip/inbound.rs:327`, "a key account's mail: an item was written", left `[1, 2]`, right `[1]` (the named catcher) |
| I5 | authentication check dropped | `ingest.rs`, the iMIP gate: `&& sender_authentication.is_authenticated()` removed (the spam-training use of the same expression untouched) | `refused_deliveries`: the no-DMARC sender | `inbound.rs:327`, "no DMARC: an item was written", left `[1, 2]`, right `[1]` (the named catcher) |
| I6 | known UID without pending items | `za_imip_uid_known`: `if pending.is_empty() {` became `if pending.is_empty() \|\| true {` (the UID index alone decides) | `known_uid_by_pending_item`; then `logged_out_request_then_cancel` | `fanout.rs:1440`, "the CANCEL's mail-borne item", left `[]`, right `[Some(Cancel)]` (Task 4's adjusted `fanout::test_plain_owner`: the CANCEL known by the pending REQUEST; Task 4's K2 is this mutation and fails `known_uid_by_pending_item`) |
| I7 | auto-add not repeated in the drain | `apply_mail`: `if !itip_auto_add_allowed(..)` became `if false && !itip_auto_add_allowed(..)` | `contact_deleted_before_drain` | `inbound.rs:850`, "a copy was imported", left `1`, right `0` (the named catcher) |
| I8 | rejected mail-borne items kept as history | Two edits in `drain.rs`, since `delete_mail` has no tree at HEAD for the brief's `rewrite_only(tree, None, 0)`: `ItemCtx::reject`'s `if self.mail { return self.delete_mail(..) }` removed, and `apply_mail`'s `AutoAddDisabled` arm `ctx.delete_mail(..)` became `ctx.rewrite_only(tree, None, 0)` | `replies` (the stranger's item still present), `contact_deleted_before_drain` | `inbound.rs:797`, "the non-participant's REPLY was not deleted" (`replies`, the named catcher) |
| I9 | mail to `ItipMessages::queue` | `za_create_event`: in the batch closure after `insert_at`, `ItipMessages { messages: za_mail.iter().map(\|m\| m.contents.clone()).collect() }.queue(&mut batch)` when `za_mail` is non-empty; the final `za_imip_submit_all(server, za_mail)` became `let _ = &za_mail;` | `mailed_once`; `leak::test_imip`: a task with the consumer held | `imip/outbound.rs:467`, "a mail recipient reached the task queue", left `1`, right `0` (`mailed_once` under the hold, the named catcher; `leak::test_imip` not reached) |
| I10 | RSVP links on | `services/src/za_imip.rs`: `rsvp: false` became `rsvp: true` | `mailed_once`: an RSVP URL in the body | `outbound.rs:257`, "no RSVP link" (the seam case, Task 2's assertion, which comes first; `mailed_once`'s own RSVP assertion is not reached) |
| I11 | plain local account left `NotInternal` | `resolve_inner`: the `!is_key_account()` return `Recipient::Mail` became `Recipient::NotInternal` | `statuses`: `5.3` on `imip-out-plain` | `fanout.rs:285`, left `["5.3"]`, right `["1.1"]` (Task 6's adjusted mixed invite: the plain recipient) |
| I12 | key account without a served key mailed | `resolve_inner`: the missing-served-key return `Recipient::NotInternal` became `Recipient::Mail` | `statuses`: `imip-pending` not `5.3` | `reply.rs:410`, left `["1.1"]`, right `["5.3"]` (an earlier module's key account without a served key) |
| I13a | mail before the commit, create | `za_create_event`: `za_imip_submit_all(server, std::mem::take(&mut za_mail)).await;` inserted after the batch is built, before the lease commit | `store_commit_failures` (create): the loser's canary at the sink | `outbound.rs:486`, "the plain attendee imports the invitation", left `2`, right `1` (`mailed_once`: plain's calendar holds two copies of the invitation instead of one once the mail goes out before the commit; the cause was not investigated). Follow-ups below |
| I13b | same, update | `update`: the same line inserted before `let committed = if fail {` of the commit loop | `store_commit_failures` (update) | `outbound.rs:879`, "the failed update mailed" (`commit_failures`, the hook case) |
| I13c | same, DELETE | `delete`: the same line inserted before the commit pause point, after the fan-out loop | `store_commit_failures` (DELETE) | `outbound.rs:905`, "the failed DELETE mailed" (`commit_failures`, the hook case) |
| I13d | REFRESH answer before its deletion | `answer_refresh`: the submission loop moved before `ctx.delete_mail("Refresh")` and made unconditional | `refresh_two_sessions`: two answers at the sink | `outbound.rs:331`, "one answer per REFRESH", left `2`, right `1` (the named catcher) |
| I14 | chunked DELETE mails | `delete`: on the commit's error path `za_imip_trace_dropped(account_id, &za_mail)` became `za_imip_submit_all(self, std::mem::take(&mut za_mail)).await` | `chunked_collection_delete`: mail at the sink | `outbound.rs:905`, "the failed DELETE mailed" (the single-event hook case shares the error path). Follow-ups below |
| I15 | local list left `NotInternal` | `resolve_inner`: `Recipient::Mail` became `Recipient::NotInternal` where `za_account_id_from_email` answers `None` | `statuses`: the list's `5.3` | `fanout.rs:289`, left `["5.3"]`, right `["1.1"]`: the literal edit also makes every outside address `NotInternal` (a list and an outside address both resolve to no account), so an earlier assertion fails. The list-only variant is below |
| I16 | token rule applied to mail recipients | `route`: `if mail {` became `if mail && false {` | `attendee_replies`: no REPLY to the outside organizer | `fanout.rs:285`, left `["5.3"]`, right `["1.1"]` |
| I17 | delegating merge mails outside delegates | the drain's delegating `za_fan_out` call: `internal_only` `true` became `false` | `outside_delegation`: `1.1` on DELEGATE or a REQUEST at the sink | `inbound.rs:938`, the status assertion (the item's `SCHEDULE-STATUS=1.1` on the DELEGATE line instead of `5.3`; the sink count cannot fail because the drain never submits its fan-out's mail, as Task 7 recorded) |
| I18 | organizer's-copy gate removed | `mail_delegates` became `matches!(incoming, Incoming::Mail { .. })` (the organizer-is-local and REPLY conditions removed) | `delegate_reply_into_delegators_copy`: an item at imip-key2 | `inbound.rs:1145`, "an attendee's copy fanned out", left `[1, 2, 4]`, right `[1, 2]` (the named catcher) |
| I19 | `From` traced | `za_imip_item`: the `ItipMessageReceived` event's `From = trc::Value::redacted()` became `From = sender.clone()` (`sender.as_str()` does not compile; the macro takes an owned value) | `imip::traces` | `imip/traces.rs:610`, "an iMIP event carries an address or content: [delivery: the REQUEST] Calendar(ItipMessageReceived): imiptrace-from-canary@imiptrace-domain-canary.remote.test" (the named catcher) |

Follow-up runs, each the row's mutation plus a tests-only edit that turns the assertion that failed first into a `let _ =` (applied and reverted with the mutation, never committed), so the next catcher in the suite shows:

| Id | Row | Tests edit | First failure observed |
|----|-----|------------|------------------------|
| I13a2 | I13a | `outbound.rs:486` relaxed | `outbound.rs:846`, "the failed create mailed" (`commit_failures`, the create hook case) |
| I13a3 | I13a | `:486` and `:846` relaxed | `outbound.rs:1025`, "the losing create mailed" (`store_commit_failures`, the lost-lease create: the named catcher) |
| I13b2 | I13b | `:879` relaxed | `outbound.rs:958`, "the losing update mailed" (`store_commit_failures`, the named catcher) |
| I13c2 | I13c | `:905` relaxed | `outbound.rs:982`, "the losing DELETE mailed its CANCEL" (`store_commit_failures`, the named catcher) |
| I14b | I14 | `:905` relaxed | `outbound.rs:982`, the same loser's CANCEL (the parked DELETE fails its assertion through the same error path) |
| I14c | I14 | `:905` and `:982` relaxed | `outbound.rs:1077`, "the failed DELETE mailed a CANCEL", left `2`, right `0` (`chunked_collection_delete`, the named catcher) |
| I15n | I15 | the edit narrowed to the list address (`address.starts_with("imip-list@")` returns `NotInternal`) | `outbound.rs:388`, `imip-list@example.com`, left `["5.3"]`, right `["1.1"]` (`statuses`, the named catcher) |

Rows whose named catcher the full-suite run did not reach (I1, I2, I3, I6, I10, I11, I12, I16) were killed by an earlier assertion that the same task adjusted or wrote; their own cases are covered by the tasks' kill checks below (Task 4's K1-K5 for I1, I2, I4 and I6; Task 11's reduced run for I3; Task 2's for I10). I11, I12 and I16 were not isolated further.

### Kill checks of Tasks 2-11

Each was made by the task's implementer against a commit, one at a time, reverted with `git checkout -- <file>` and followed by an empty `git status --short crates/`; none was committed. Logs are in the git-ignored workspace.

| Task | Mutation | Caught by |
|------|----------|-----------|
| 2 | `ImipComposer::submit`: the `options.rsvp &&` guard before `set_recipient` dropped | `outbound.rs:79` "no RSVP link" (`t2-kill-rsvp.log`, 56 s); this closed Task 1's ruling |
| 2 (fix round) | `services/src/za_imip.rs`: `SenderLookup::Quiet` became `Traced` | `outbound.rs:156`, "the submission traced the sender's address outside the transit" (`t2f1-kill-quiet.log`, 59 s) |
| 2 | `za_redact: true` became `false` on the iMIP local session | **No test can fail: an equivalent mutant**, recorded in the residuals |
| 4 | K3 the method filter disabled (`if false && !matches!(..)`) | `refused_deliveries`, "PUBLISH: an item was written" (`t4-kill-method.log`) |
| 4 | K1 the key-sender refusal falls through | "a key account's mail: an item was written" (`t4-kill-keysender.log`) |
| 4 | K4 the auto-add refusal falls through | "an unknown sender's new UID: an item was written" (`t4-kill-autoadd.log`) |
| 4 | K5 `EventNotFound` for a non-REQUEST unknown UID falls through | `unknown_uid_other_methods`, "CANCEL: an item was written" (`t4-kill-notfound.log`) |
| 4 | K2 `za_imip_uid_known` ignores pending items | `known_uid_by_pending_item` (`t4-kill-pending.log`) |
| 5 | K1 `snapshots.organizer.email.is_local` dropped from `mail_delegates` | `delegate_reply_into_delegators_copy`, "an attendee's copy fanned out" (`t5-kill-d15gate.log`) |
| 5 | K2 the REFRESH answer submitted before `delete_mail("Refresh")` | `refresh_two_sessions`, "one answer per REFRESH", `2` vs `1` (`t5-kill-refresh-order.log`) |
| 5 | K3 `reject`'s mail branch removed (kept as history) | `replies`, "the non-participant's REPLY was not deleted" (`t5-kill-reject.log`) |
| 6 | the `statuses` case written before the implementation | `outbound.rs:320`, `imip-att@remote.test` read `5.3`, not `1.1` (`t6-red-za.log`) |
| 7 | the submissions removed (RED by mutation) | `fanout::test` fails |
| 7 | the drain's delegating fan-out passes `internal_only: false` | `outside_delegation` at its status assertion, `inbound.rs:938` (`t7-kill-f5.log`); the sink assertion cannot fail since the drain never submits its fan-out's mail |
| 7 (fix round) | the abandonment guard never armed; `Drop` tracing nothing | the unit test `an_armed_drop_traces_the_unfinished_events_and_a_disarmed_one_nothing` fails on each |
| 9 | the mail arm turns a merge's actions into `NothingToMerge` | the full suite fails first in Task 5's `logged_out_request_then_cancel` (`inbound.rs:820`, `t9-kill-merge.log`); the oracle alone fails at `oracle.rs:642`, "inbound step 2: inbox items", plain `1`, key `0` (`t9-kill-merge-oracle-only.log`) |
| 10 | `From` traced (I19) | `traces.rs:544` (`t10-kill-from.log`), before the fix round |
| 10 (fix round) | (a) delivery's sender lookup (`imip.rs:244`) and (b) the drain's (`drain.rs:840`) swapped for the traced `account_id_from_email` | both fail at `traces.rs:851`, "a lookup of the sender other than the session's traced its domain" (`t10f1-kill-delivery.log`, `t10f1-kill-drain.log`); (c) with (a) and the domain assertion non-fatal, the key-sender assertion fires alone (`t10f1-kill-key1.log`) |
| 11 | `changed_by` `CalendarAddress(sender.clone())` (I3) | full suite: `fanout.rs:1272`; reduced suite (server setup plus `leak::test_imip`): `leak.rs:2837`, "inbox item 3/1: a row that names an address" (`t11-kill-full.log`, `t11-kill-leak.log`) |

Tasks 1, 3 and 8 recorded no kill check: Task 1 and Task 3 are unit-tested extractions and formats whose suite-level guards are Task 2's seam case and Tasks 4 and 11, and Task 8 changed nothing.

## Other harnesses

Task 8's four records (audit by reading, then the runs; the audit's file:line evidence is in the Task 8 report in the git-ignored workspace). None of the four routes mail to the sink, none changed, and no `route_outside_mail` call was added.

1. **Key-mode `webdav_tests`.** Not routed. The RFC 4791 sample events in `cal_query.rs` name organizers and attendees (`cyrus`, `lisa`, `bernard`) who are not the account that stores them, and a `VFREEBUSY` is not a scheduling object, so upstream's scheduling builds no message; `za_variants::scheduling` invites between two key accounts only (the mail class is empty, the test asserts the item and no mail); `cal_alarm` and `cal_scheduling` are replaced by their key variants in key mode. The suite's closing `assert_is_empty` treats a `QueueMessage` as a leftover (`tests/src/utils/cleanup.rs`), so a queued message would fail it.
2. **Key-mode `imap_tests` and `jmap_tests`.** Not routed. No `text/calendar` part or scheduling write: key-mode JMAP skips the `calendar::*` modules and answers `accountNotSupportedByMethod` for calendar methods; the one fixture with a `text/calendar; method=REQUEST` part is read only by the `compliance` suite, which key mode skips and plain mode imports with `Email/import`, not over SMTP.
3. **The independent CalDAV client.** Not routed. Every invitation names the harness's own users (`alice`, `bob`, `carol`), key accounts in key mode, so the mail class is empty; the pinned python-caldav suite's outside addresses are unused, unsaved or organizer-less. A read-through, not a run: no local Python has the packages (`caldav`, `icalendar`, `pytest`, `niquests`, `lxml` and the rest of `requirements.txt`), nothing was installed (ruling P5), so the plain and key runs are **pending for CI's `caldav-client` job**. Expected there from `docs/zero-access/client-caldav.md`: key mode 1 skipped and 2 xfailed, plain mode with its five records. `runner.py` needs `--caldav-source` as a git clone checked out at the pin.
4. **The fresh-store gate.** Not routed: `tests/product/fresh_store.py` writes no attendee or organizer. It passes on a built binary (below).

Task 8's runs, repeated on the head `f13e0075a` in Task 12 (all pass, results in the table under "Where things stand"): key-mode `webdav_tests` 21.85 s (the closing `destroy_key_accounts` and `assert_is_empty` hold, so no `QueueMessage` is left), key-mode `imap_tests` 12.24 s, key-mode `jmap_tests` 91.06 s, and the fresh-store gate on the shipping fat-LTO binary: the nine failure tests, then `PASSED: 9 readback checks after the restart`. At Task 8's head (`db5bfbb0e`) the same runs took 22.54 s, 11.44 s and 92.13 s, and the gate ran on a thin-LTO local binary (6 m 49 s build, ruling at Task 8). The CalDAV client's plain and key runs remain pending for CI's `caldav-client` job; the controller raises them with Jay at the finish.

## Adjusted assertions

Existing assertions that failed only because a recipient is now mailed or an item is now written, each adjusted with a comment citing the spec section (spec 5.4, 4.1, 4.5 or 8.5):
- **Task 4.** `fanout::test_task_queue`: "nothing in key2's inbox" became one new pending mail-borne item (row `CalendarAddress("")`, sender `plain@example.com` in the opened carrier), deleted at the module's end. `fanout::test_plain_owner` (spec question 1): after the PUT exactly one mail-borne pending REQUEST, after the held DELETE exactly one pending CANCEL, both deleted. `gating::test_scheduling`: the plain organizer's invitation adds one mail-borne pending item (the DAV listings of the inbox and calendar stay empty); every item since the test's start is deleted at its end. `mail::fence::forward_race_imip_side_write`: the sealed retry asserts no `CalendarEvent` and every inbox item present pending with a `WRAP_PK` envelope, true whether or not the queue's retry ran (fix round 1 of Task 4; the helper keeps its name `assert_no_calendar_data` for the weekly inventory gate, ruling at Task 4).
- **Task 5.** `gating::test_scheduling`: the drain block is new (copy after the drain, the CANCEL then ends `EventNotFound`); no existing assertion was removed or loosened.
- **Task 6.** `fanout.rs` mixed invite: `plain@example.com` and `outsider@remote.test` read `1.1` (the sender's sub-address and key3 without the permission stay `5.3`); `sharing.rs` `sole-group` and `delegated_identity_and_groups`' `WRITER_GROUP` read `1.1` (a plain group is the mail class, decision 10); `tracing.rs`'s `LOOKUP_LOCAL_ATTENDEE` and `SUBADDR_ATTENDEE` read `1.1`; the nine direct `za_fan_out` calls of `sharing.rs` gain `internal_only: false`.
- **Task 7.** `imip::outbound::seam` clears the sink first (the `imip::inbound` PUTs inviting ATT are mailed); `fanout::test` counts plain's mail (`plain_mail + 1`, the DELETE's CANCEL moves the count by one) and ends with `clear_received_scheduling(plain)`; `gating::test_scheduling` counts plain's mail and the planted invitation's CANCEL and ends with `clear_received_scheduling`; `tracing::test` splits its span rule by commit window (below); `leak.rs`'s `SCHED_EVENT` marks the plain ATTENDEE `SCHEDULE-AGENT=CLIENT` and purges queue blobs before the scans of `test_drain` and `test`; `pinning::case_11_mixed` waits for the queue and asserts the outside address's mail; `sharing.rs` ends the lifecycle group's module with `clear_received_scheduling_as`.
- Tasks 1, 2, 3, 9, 10 and 11 changed no existing assertion (Task 3 added a sixth argument `None` to existing `seal_pending_item` calls in unit tests).
- `expire_and_purge_blobs` moved from `mail/fence.rs` to `tests/src/utils/za.rs`, with a function rename entry in `tests/qualification/dispositions.json` for the weekly inventory gate (Task 7); `assert_no_calendar_data` was restored to its name, so no other `tests/src` function listed in the baseline was renamed or removed.

## Spec notes

What execution found that the spec, the plan or a brief had wrong or missing. None changes a decision of the spec.
- **Spec 4.2 erratum.** The root holds "exactly one of" the three carriers; release 2 fan-out items legitimately carry none (a non-organizer writer's copy, a removed attendee with no stored pair), and the drain handles them. The text should read: "at most one; a mail-borne item carries exactly `X-ZA-SENDER`" (Task 3 ruling).
- **Spec 4.1 step 6** and older notes name `write_item_inner`, now `za_write_pending_item` (Task 3 renamed it); the spec was not edited.
- **Spec 8.5's tracing rule.** The submission's cache lookups do not carry its SMTP span id: upstream's `rcpt_resolve` emits `Cache(Hit|Miss)` events naming the domain or local part with no `SpanId`, and so does the queue's route evaluation. The behaviour is the spec's (section 7: the SMTP layer traces the envelope as transit metadata; the fork's lookups stay quiet); only the test's way of recognising transit events changed. `tracing::test` and `imip::traces` split by commit window: strict before the parked commit; after it, `Smtp`, `Delivery` and `Queue` events, their spans and `Cache` events are allowed, and an `ItipMessage*` event is never exempt (Task 7 and Task 10 rulings).
- **Spec 8.5's `task9-unknown` premise.** `task9-unknown@not-local.test` is the organizer of a planted pending item the drain refuses; no write in that case builds a message for it, so nothing is mailed (Task 7).
- **Spec 8.2's `attendee_replies` order.** Upstream's DELETE of an already declined copy sends no REPLY (`attendee_decline` skips DECLINED without `SCHEDULE-FORCE-SEND`), so the case runs DECLINED, ACCEPTED, DELETE (Task 7).
- **Spec 7's D17 table has no row for the abandonment trace** (`ZA_IMIP_ABANDONED`, Task 7's fix round), and 5.4's residuals omit the disconnect case; both are recorded in the residuals below.
- **Spec 5.1 and a `Failed` lookup.** A `Failed` recipient lookup on a route release 2 does not attempt records `5.3`, not 5.1's `5.1` (`fanout.rs`), for example a key attendee's REPLY to an outside ORGANIZER under a transient store error. Nothing is mailed either way (Task 6, deferred to the final fix wave).
- **Spec 8.2's plain local attendee check.** Only ATT's sink copy is checked for the missing RSVP links; `imip-out-plain`'s delivered HTML is not read (Task 7, deferred to the final review).
- **Task 12's brief at HEAD.** `za_imip_sender` mirrors the sender closure at `ingest.rs:546-555` (the brief and spec cite `:527-536`); the brief's mutation rows I8, I9, I13a-d and I15 describe code by intent and HEAD's shape differs (see the mutation table); `imip::traces` does not disable the recipient-keyed inbound throttles (only `imip::inbound` and `imip::oracle` do, and `traces` delivers a handful of messages to a recipient of its own); Docker is not installed on this machine.

## Accepted residuals

None exposes content or a key beyond what the spec accepts. By source:

**Spec 5.4 (outbound mail after the commit; none is retried).**
1. A submission that fails after the commit is traced (`ItipMessageError`) and leaves `1.1` in place, with no corrective write.
2. A recipient the local SMTP layer refuses after the commit (a typo on a local domain, a disabled address, an RCPT-stage milter) keeps `1.1`, which a client shows as sent; upstream leaves the property bare.
3. A crash between the commit and the submissions loses those emails silently. Beside it, **a client disconnect between the commit and the end of `za_imip_submit_all`** (hyper drops the request future) cancels the remaining submissions; since Task 7's fix round that loss is traced once per distinct event with `ZA_IMIP_ABANDONED` (the in-flight attempt included, its outcome unknown), and the mail is still lost. Detaching the loop (`tokio::spawn`) was not taken: it rewords invariant 41 and Jay's "composed in the user's request" decision; the alternative is raised with Jay. Two post-commit await points precede the guard (the create's lease release and the DELETE's `invalidate_caches`), deferred to the final fix wave.
4. A collection DELETE whose commit fails in a later chunk loses the CANCELs of the events an earlier chunk already deleted (the store returns only the error); traced as `ZA_IMIP_DROPPED`.
5. Nothing is retried (there is no task). A DELETE's lost message (the organizer's CANCEL, the attendee's declined REPLY) is permanent: the event is gone, so no later change exists, and the outside parties keep a stale state. Upstream's task retries for three days and commits with the event.

**Spec 4.6 (abuse bounds).**
1. A non-participant's mail-borne items: delivery checks participation only in the message itself, so any sender upstream's `is_authenticated()` accepts that knows a UID (an event's or a pending item's) can write mail-borne items. They occupy pending slots of a logged-out account until its next session, never evicted; at the limit each evicts the oldest drained history, and once only pending items remain other deliveries are refused (release 2 items record `5.1`). A non-participant can therefore evict drained history, which upstream's ingest-time check against the stored event does not allow.
2. A retried delivery's second item: key accounts skip the thread lookup, so ingest's SMTP duplicate check never fires; a retried REQUEST yields a second drained notification (upstream's REQUEST merge always records `Sequence`), and a duplicate REPLY carrying `REQUEST-STATUS` or a repeated CANCEL produces actions even when nothing differs. Each item counts toward the notification limit until drained.

**Spec 3.2 (inherited limitations; upstream 1.0.1 behaviour recorded instead of corrected).**
1. DMARC is the basis of trust for outside senders.
2. With auto-add off, an invitation for an unknown UID from a sender that is neither a local principal nor in a ContactCard reaches the mailbox only (`AutoAddDisabled`).
3. A REQUEST, ADD or CANCEL must come from the ORGANIZER's address: an assistant sending with `SENT-BY`, or a delegator forwarding a REQUEST (upgrade ruling 166), is refused.
4. PUBLISH, COUNTER and DECLINECOUNTER are never applied (`UnsupportedMethod`).
5. A delivery the queue retries re-runs the iMIP processing; an ordinary account's duplicate check stops it, a key account's does not (see 4.6, 2).
6. A CANCEL copies each ATTENDEE line with its `SCHEDULE-STATUS` (upgrade ruling 109), and the CANCEL to a removed attendee carries a summary of the new event (ruling 113); both now reach outside mailboxes.
7. An RSVP link in an organizer's email (a plain local organizer's, or a provider's own buttons) updates only the organizer's copy, never the key account's.
8. No `SCHEDULE-STATUS` follows a mail's fate; a remote bounce reaches the sender's mailbox as a DSN.
9. A REPLY is applied whatever its SEQUENCE, so a reply that crossed a reschedule in the mail updates PARTSTAT (D6, withdrawn).
10. `must_match_sender` compares MAIL FROM exactly with the account's addresses: a key attendee invited at a sub-address (`key1+tag@example.com`) replies from it, and the REPLY is refused at MAIL FROM and never sent, as upstream's task refuses it.
11. A grantee who organizes under its own address from an owner's shared calendar receives outside REPLYs into its own account, whose UID index has no such event (`EventNotFound`), as upstream's `itip_ingest` fails.
12. `sender_is_organizer_or_attendee` accepts the ORGANIZER as a REPLY sender, so an outside or plain-local organizer's own REPLY reaches the drain and, on the attendee's copy, ends as `UnsupportedMethod`, deleted (D7).
13. An authenticated local submission is trusted on its envelope: with `must_match_sender` off, any authenticated local account can send iMIP from any address, as upstream's ingest trusts it.

**Jay's decision 11 (issue #72).** A store writer can forge a whole mail-borne item, sender included, with no SMTP delivery: at the user's next session a forged REFRESH naming a listed attendee makes the drain mail the event's current full state through the SMTP queue (D14), and a forged delegating REPLY from a listed attendee makes upstream's merge add delegates, whom the drain invites at once (D15). Both need a listed attendee that is not a local key account; a store writer that remaps or disables a key attendee's address brings an all-key event within the exposure. Issue #72 (verify DKIM provenance in the drain) owns the follow-up and its qualification test; no test forges an item, and findings about forged items are out of scope for review.

**Spec 5.7.** Events created before release 5 keep their `5.3` for outside and plain-local attendees; nothing re-invites automatically. Those attendees are mailed on the next change that builds a message for them, and an edit that builds none (an alarm, a change without scheduling content) mails nobody.

**Recorded by the ledger (`residual`, and the items routed to this note):**
- Task 3 (the spec 4.2 erratum, from its ruling): the root holds **at most one** of `X-ZA-RSVP-TOKEN`, `X-ZA-EXTRA` or `X-ZA-SENDER`; a mail-borne item carries exactly `X-ZA-SENDER`. Release 2 fan-out items legitimately carry none.
- Task 2: residual (for the outcome note): za_key_rcpt: true on the iMIP local session is an equivalent mutant at release 5 — reads at session.rs:657 (stream loop only), reset (stream callers only), data.rs:591 and spam.rs:100 (spam filter off for authenticated sessions, structs_impl.rs:48005-48007), milter/message.rs:46 (no milter configured), scripts/exec.rs:166 run_script (stage scripts default "false"); rcpt.rs:408 and data.rs:808 only skip a quiet lookup (the flag is also set on a lookup error, rcpt.rs:430; a quiet lookup can emit domain-id cache events, principals.rs:172,177, carrying no address); every redaction read ORs za_key_sender(), already true for the key owner. D12 holds as defence in depth.
- Task 6: residual (for the outcome note, and raise with Jay at the finish): a pinned or withheld (3.8) address that a store writer or admin removes or remaps to a plain principal is now mailed in plaintext on the key organizer's next edit, the pin never consulted (spec 5.2 "mail recipients take no part in key pinning"; spec section 2 accepts store-writer remapping inside decision 11's paragraph) — key pinning spec 4.3's remap attack ending in mail instead of a fresh key.
- Task 8: residual (for the finish, P5): CalDAV client runs pending for CI's caldav-client job.
- Task 5: minor (deferred → outcome note, Task 12): an ADD in RFC 5546 3.2.4's own form (DTSTART, no RECURRENCE-ID) never adds an instance (upstream adds only RECURRENCE-ID-keyed instances, scheduling/inbound.rs:208-227) — inherited limit missing from spec 3.2's list; the test exercises the RECURRENCE-ID form.
- Task 5: minor (deferred → outcome note): a delegate's REPLY that names only itself never reaches the delegator's copy (upstream's itip_snapshot refuses a message naming none of the recipient's addresses) — inherited limit beside 3.2 item 12.
- Task 7: minor (deferred → outcome note; carried to Task 10): tracing::test's post-commit window exempts every Cache event, so a future recipient-keyed fork lookup after the commit goes unseen (spec 8.5 line 972's span rule cannot hold: upstream's rcpt_resolve Cache events carry no SpanId); Task 10 could add a count-based differential against an ordinary organizer's submission to the same addresses.
- Task 7: minor (deferred → outcome note): spec 8.5 line 980's premise that task9-unknown@not-local.test is mailed is false (it organizes a planted item the drain refuses); optional sink count == 0 guard.
- Task 7: minor (deferred → outcome note / spec): the abandoned trace has no row in spec 7's D17 table, and 5.4's residuals omit the disconnect case (record beside the crash).

## Deferred findings

What stays open after this plan. Each is a ledger line (`minor (deferred ...)`), verbatim, grouped by task; the arrow names where the ledger routed it (the final review, this note, a later task). Task 8 and Task 12 have none.

### Task 1

- Task 1: minor (deferred): trc::location!() of the moved error traces names new lines (logo, account_info, sender lookup, ThreadError); the sent/error events carry none.
- Task 1: minor (deferred): send_imip's events now emitted in the worker after the join, not in the detached task; a JoinSet dropped at shutdown after queueing loses ItipMessageSent (shutdown-only).
- Task 1: minor (deferred): `is_data = rejection.stage == "DATA"` selects the event key order by magic string (imip.rs:138); a typed stage would be safer.
- Task 1: minor (deferred): compose_imip_message takes six positional &str; the unit test pins the builder, not submit's call-site mapping (verified correct today).
- Task 1: minor (deferred, plan-mandated): ImipComposer::submit takes server and context again, separately from the ones the composer was built from.
- Task 1: minor (deferred, plan-mandated): four itip_* functions exposed where spec 3.1 item 2 names two plus the predicate (itip_fill_organizer public on its own).
- Task 1: minor (deferred, plan-mandated): extraction tests cover REQUEST/REPLY only; REFRESH/COUNTER/ADD/CANCEL/PUBLISH/DECLINECOUNTER sender-rule arms, the REQUEST fill and itip_auto_add_allowed have no unit test.
- Task 1: minor (deferred): za_imip.rs module doc describes the submitter before Task 2 adds it (moot after Task 2).

### Task 2

- Task 2: minor (deferred): za_imip.rs failed() attaches caused_by(location!) inside the helper, so the wrong-recipient-count and unparseable-summary failures trace identically (:115-122, :74, :86).
- Task 2: minor (deferred, plan-mandated): the seam's hook step comment says "traced, nothing sent" but asserts only Failed; ZA_IMIP_FAILED's trace is first asserted in Task 7.
- Task 2: minor (deferred): ZA_IMIP_UNREGISTERED branch untested (process-wide OnceLock); the report's "unregistered answers Failed" is by reading only.
- Task 2: minor (deferred, plan-mandated by spec question 4): the dummy instance's span_id_gen is independent of inner.data.span_id_gen and the listeners'; same-millisecond collisions possible (upstream shares the property).
- Task 2: minor (deferred): a ZaImipMail with zero or several recipients only traces and fails; a debug_assert! would make a caller bug loud in test builds.
- Task 2: minor (deferred, from re-review): the trace check's positive control shows only that transit events were captured, not that non-transit Cache events reach the collector (outbound.rs:141-159); the one-off kill run proves it today — an assertion that some non-transit Cache event was captured would guard it.

### Task 3

- Task 3: minor (deferred, plan-mandated; for the final review): za_write_pending_item takes changed_by and sender independently (fanout.rs:1491-1503, :1573): a sender with PrincipalId, a mail-borne call with sender None, or a non-empty CalendarAddress (an address in the visible row, invariant 43/D4) are all accepted; mail-borne "exactly one" and D4's empty row rest on Task 4's single caller and mutations I2/I3 — a by-construction origin enum would hold it.
- Task 3: minor (deferred): the X-ZA-SENDER entry is read with entry_text (first value), parameters and extra values not refused (item.rs:348-353); validate_bundle_carrier (tree.rs:162) would give parity with event roots; decision 11 scopes out forged items.
- Task 3: minor (deferred): Some("") is sealed and opens as Some(""); unreachable from upstream's sanitize_email (utils lib.rs:264-289).
- Task 3: minor (deferred, plan-mandated): the_drained_form_carries_no_sender can hardly fail (a leftover sender entry would be sealed into X-ZA-SEALED); the round trip's assert_eq!(ical, tree()) covers it; open_drained_item + equality would be the stronger form.
- Task 3: minor (deferred): untested: a non-UTF-8 sender → Format, two X-ZA-SENDER entries, the tree unchanged after the seal-time sender refusal (all correct by inspection).

### Task 4

- Task 4: minor (deferred → Task 10): inbound oversized case cannot fail if step 1 (imip.rs:194) is removed (the writer's own >= check refuses with Ok(false)); Task 10's trace assertion should require Limit/Size and the absence of ZA_IMIP_ITEM_REFUSED for that delivery.
- Task 4: minor (deferred): unknown_uid_other_methods' REPLY case: ATT has no ContactCard, so auto-add would also refuse it; only the CANCEL case kills a dropped EventNotFound branch (K5).
- Task 4: minor (deferred, plan-mandated): za_imip_sender copies upstream's sender closure (ingest.rs:546-555) verbatim; spec 10 records it as a silent dependency; upstream's closure could call it instead.
- Task 4: minor (deferred): one undecodable notification row or pending content makes za_imip_uid_known fail, dropping later unknown-UID deliveries as ZA_IMIP_LOOKUP_FAILED until the drain removes it (store corruption only).
- Task 4: minor (deferred, plan-mandated): ZA_IMIP_LOOKUP_FAILED's text ("sender lookup failed") is also emitted for UID-index, pending-scan and za_is_key_account failures.
- Task 4: minor (deferred): OtherSchedulingAgent traced as an ItipMessageError refusal where upstream suppresses it silently (trace volume only; spec 7 arguably requires it).
- Task 4: minor (deferred, from re-review): the new check enumerates items through the Created index (inbox_records skips an index entry with no archive); a notification archive without its index would go unseen (upstream's insert cannot produce one); za_has_documents(CalendarEventNotification) == !items.is_empty() would close it.
- Task 4: minor (deferred, from re-review): the helper's doc says "sealed to the served key" but checks only the wrap-type byte; "pending, with a pk wrap" would be accurate.

### Task 5

- Task 5: minor (deferred → final review / Task 12 mutation list): D7's NothingToMerge deletion (drain.rs:1439) and the drain's own EventNotFound (:889-891, merge's missing archive :1321) have no case; a keep-as-history mutation of either survives. Cheap cases: an RFC 5546 3.2.4 ADD without RECURRENCE-ID (MergeResult::None); a CANCEL for a known UID whose event is deleted with Schedule-Reply: F before the drain.
- Task 5: minor (deferred → outcome note, Task 12): an ADD in RFC 5546 3.2.4's own form (DTSTART, no RECURRENCE-ID) never adds an instance (upstream adds only RECURRENCE-ID-keyed instances, scheduling/inbound.rs:208-227) — inherited limit missing from spec 3.2's list; the test exercises the RECURRENCE-ID form.
- Task 5: minor (deferred → outcome note): a delegate's REPLY that names only itself never reaches the delegator's copy (upstream's itip_snapshot refuses a message naming none of the recipient's addresses) — inherited limit beside 3.2 item 12.
- Task 5: minor (deferred): find_event errors in the mail arm keep release 2's full store error while delivery cuts UID-keyed lookup errors to their type; ZA_IMIP_LOOKUP_FAILED's doc says "sender's address or the UID" (document_ids_matching attaches no key context).
- Task 5: minor (deferred → Task 7): outside_delegation's sink().count_to(DELEGATE) == 0 cannot fail before Task 7 submits mail; kill check after Task 7: internal_only false in the drain's delegating block must fail it.
- Task 5: minor (deferred): set_drain_retry_override(Some(0)) in outbound.rs:155/:167 cleared only on the success path; a drop guard would match globals' hook rule.

### Task 6

- Task 6: minor (deferred → Task 7): nothing in Task 6 asserts ZaFanOutResult.mail (a dropped push, the unreconciled message or a push under internal_only pass Task 6's tests); Task 7's sink cases cover it end to end, plus Task 5's carried kill check (drain internal_only false must fail outside_delegation).
- Task 6: minor (deferred): stale comment fanout.rs:425-427 (the loop adds the ORGANIZER to recipients at first appearance, to internal only on Deliver).
- Task 6: minor (deferred, plan-mandated): resolving every address runs try_account, access_token build and a served-key vault read for internal addresses on never-attempted routes (cost and trace noise only).

### Task 7

- Task 7: minor (deferred → outcome note; carried to Task 10): tracing::test's post-commit window exempts every Cache event, so a future recipient-keyed fork lookup after the commit goes unseen (spec 8.5 line 972's span rule cannot hold: upstream's rcpt_resolve Cache events carry no SpanId); Task 10 could add a count-based differential against an ordinary organizer's submission to the same addresses.
- Task 7: minor (deferred): tracing windows assigned after fixed 500 ms sleeps, not a barrier (collector lag → a pre-commit Cache event tagged PostCommit; a false pass only).
- Task 7: minor (deferred → outcome note): spec 8.5 line 980's premise that task9-unknown@not-local.test is mailed is false (it organizes a planted item the drain refuses); optional sink count == 0 guard.
- Task 7: minor (deferred, plan-mandated): the_second_chunk_fails_once_for_its_account's other-account control cannot fail (OTHER's batch is one chunk; slot survival unchecked).
- Task 7: minor (deferred → final review): spec 8.2 "the plain local attendee gets the invitation without RSVP links" — only ATT's sink copy is checked; imip-out-plain's delivered HTML is not read.
- Task 7: minor (deferred): commit_failures shows no positive control that its own event produced a mail attempt.
- Task 7: minor (deferred): fanout.rs:303-309 plain_new.iter().all(..) passes when empty.
- Task 7: minor (deferred → final review): za_commit_create maps za_finish_create_loss's Result<HttpResponse> with .map(|_| ()); correct only because that function never returns Ok — now load-bearing for invariant 46; return DavError / Infallible.
- Task 7: minor (deferred): update and delete commit hooks leave a constant-false branch with the hook's error string in the shipping source (the create's #[cfg(feature = "test_mode")] form removes it).
- Task 7: minor (deferred, plan-mandated → Task 11): the leak modules' in-place purge expires every temporary link before the scans; a key-account path leaving plaintext under a temporary link would be purged, not reported (none runs there today).
- Task 7: minor (deferred → Tasks 9-11): throttles stay on in imip::outbound (~16 messages to ATT after ~4 from imip::inbound, budget 25/h per sender domain and recipient); later modules mailing ATT must disable the Rcpt-keyed throttles or budget.
- Task 7: minor (deferred): SECOND_CHUNK_FAIL_ONCE armed after wait_for_queue only; an index pass under ForceCommitSplit could consume it (a loud flake); wait_for_index_queue first.
- Task 7: minor (deferred → final fix wave): two post-commit await points precede the guard — the create's lease release (update.rs:1536; za_commit_create mod.rs:703) and the DELETE's invalidate_caches (delete.rs:592, grantees only); a drop there loses the mails untraced; arm the guard when the commit returns Ok and pass it into za_imip_submit_all (do not move the submission before the release or the invalidation).
- Task 7: minor (deferred): the in-flight attempt may already be queued when the future drops, yet is traced ZA_IMIP_ABANDONED (plan-mandated "in-flight included"); the constant's doc could say its outcome is unknown.
- Task 7: minor (deferred → outcome note / spec): the abandoned trace has no row in spec 7's D17 table, and 5.4's residuals omit the disconnect case (record beside the crash).
- Task 7: minor (deferred): the guard's unit test skips remove_subscriber on a mid-test failure and blocks the current-thread runtime with std::thread::sleep (harmless).

### Task 9

- Task 9: minor (deferred → final review): the copy's location (calendar, resource name) is never compared; copy_href finds it by UID across calendars; compare both hrefs through normalize.
- Task 9: minor (deferred): Kind::Header unfolding deletes folding whitespace (RFC 5322 removes the CRLF only); harmless today.
- Task 9: minor (deferred, plan-mandated by 8.3's part list): parts beyond the list (application/ics attachment, logo, content-type tree, duplicate headers) are not compared; both paths share compose_imip_message.
- Task 9: minor (deferred): DTSTAMP rule replaces the whole line by prefix (parameters, longer names).
- Task 9: minor (deferred → final review): rcpt_throttles/set_throttles copied verbatim from inbound.rs (oracle.rs:831-862); move to imip/mod.rs.
- Task 9: minor (deferred): the kill check shows the count assertion only; the stored-event line's firing is inference (by reading it holds: SEQUENCE 0 on the key copy).
- Task 9: minor (deferred → final review / mutation list): no self-check covers mail_parts' method → kind choice (oracle.rs:787-791); reading every sink ICS as IcsCopy would pass the self-check and the live exchange, reopening F1's blind spot; build REQUEST and CANCEL SinkMessages in self_check and assert the kinds.
- Task 9: minor (deferred): the key side's kind is discarded (:749); safe while pairs align by (rcpt, method).
- Task 9: minor (deferred): the strict combined negative (:1014-1019) is already unequal on SCHEDULE-STATUS alone (harmless).

### Task 10

- Task 10: minor (deferred → final review): From required present only on ItipMessageReceived; D17 lists From [redacted] on the delivery refusals and the size row (assert has(Key::From) on NOT_TAKEN and too-large).
- Task 10: minor (deferred): the step-4 reason compared with the constant ZA_IMIP_KEY_SENDER instead of the spec's literal "zero-access: iMIP from a key account".
- Task 10: minor (deferred): windows assigned by fixed 500 ms settles (collector lag → a pre-commit event into Open; a false pass).
- Task 10: minor (deferred → final review): traces.rs:365-369 and :826-827 attribute the exempt event to "MAIL FROM is_local_domain"; it is DATA's DKIM signing-domain default (data.rs:863-866, structs_impl.rs:47749) — cite the real site.
- Task 10: minor (deferred): Window::Other's doc mentions flushes between steps that the fix removed.

### Task 11

- Task 11: minor (deferred → final review): carrier_controls tests one two-carrier order only ([EXTRA, SENDER, KEY]); no [SENDER, EXTRA/RSVP, KEY] or principal-row variant (flagged by code reading, unproven by a control).
- Task 11: minor (deferred → final review): parameters on an item's popped carriers are never checked (X-ZA-SENDER;CN=<address>:<bundle> passes check_item; pre-existing for X-ZA-KEY/EXTRA/RSVP on items).
- Task 11: minor (deferred): the "sender in the row" assertion (leak.rs:2842-2845) cannot fail after :2837; the scan's decoded-archive search delivers the spec line.
- Task 11: minor (deferred): the sink is counted by recipient only (not method/UID) in test_imip.
- Task 11: minor (deferred): token/carrier matching in mail is literal (mid-stream base64url, raw bytes, folded lines, RFC 2047, lower-case x-za-).
- Task 11: minor (deferred): a 253-254-octet sender (two PAD units) would be a false failure of the one-unit rule.
- Task 11: minor (deferred): the stored-trace decision (none stored) lives only in the report; a comment in test_imip would name the dependency.
- Task 11: minor (deferred): content in a task of another type would be consumed before the store-wide scan; a TaskQueue canary search while held would cover invariant 41 for every type.
- Task 11: minor (deferred): three sealed iMIP stubs stay in imip-leak-key's mailbox until destroy_key_accounts (as in the sibling imip modules).

**The plan 13 note's open questions, unchanged:**

1. **Key accounts cannot create a JMAP `FileNode` with content.** This is the spec's design, not a ruling of this plan: spec 5.7 says a sealed source must never become a file's blob (file readers have no unsealing step), and the spec 9 table's raw-consumer row refuses `FileNode/set` and `FileNode/copy` for any key-account upload. Every upload of a key account is a sealed source, so a key account can create folders but no file through JMAP; files over WebDAV are a separate path and plaintext in this release. `za_variants::file_node` asserts the refusal and writes its file over WebDAV; the refusal is Task 6's (`b869ae62`), as spec 5.7 and the spec 9 table's raw-consumer row require. The decision for spec 5.7 and 9: keep the refusal, or seal files too (a later release that gives `FileNode` a sealed blob path)? The decision is tracked as issue #50 (https://github.com/jaylaney/stalwart/issues/50, "File encryption at rest", milestone "Release 8: file encryption").
2. **The session object's advertised sort options.** Should a key account's JMAP session drop `from`, `to`, `subject` and `sentAt` from `emailQuerySortOptions` (per-account trimming in the session builder), or stay as it is while the server answers `unsupportedSort`? A client that trusts the session object would otherwise offer sorts the server then refuses.

## Merge hotspots

Spec 10's list, with Task 12's additions. Fork insertions in upstream files: `crates/email/src/message/ingest.rs` (the iMIP gate without `!za_key`, the call to `za_imip_item`); `crates/services/src/task_manager/tasks/imip.rs` (the extracted `ImipContext`, `ImipComposer` and `compose_imip_message`: a change to the composer flows through the submitter, a change to its loops needs the extraction redone); `crates/groupware/src/calendar/itip.rs` (the functions extracted from `itip_ingest`); `crates/services/src/lib.rs` (the registration line); `crates/dav/src/calendar/update.rs` and `delete.rs` (already merge hotspots: each submits the fan-out's returned mail attempts after its commit returns `Ok` and gains its commit-failure hook); `crates/common/src/za/mod.rs` (declares `imip`); `crates/store/src/dispatch/store.rs` (a test-mode line in `write_batch` for `SECOND_CHUNK_FAIL_ONCE`, beside the fork-owned `za_test.rs`). Fork-owned files that changed: `fanout.rs`, `drain.rs`, `seal/item.rs` and the new `crates/email/src/message/za/imip.rs`, `crates/common/src/za/imip.rs` and `crates/services/src/za_imip.rs`.

Silent dependencies on upstream code:
- The drain's mail arm mirrors `itip_ingest`'s order (UID lookup, auto-add, a REQUEST-only import, `MergeResult::Message` for REFRESH), as `import` and `merge` mirror `commit_itip_merge`: a change there needs a re-read of both.
- Delivery's method step relies on upstream's merge applying only REQUEST, ADD, CANCEL, REPLY and REFRESH (`crates/groupware/src/scheduling/inbound.rs`).
- The delivery sender (`za_imip_sender` mirrors the closure at `ingest.rs:546-555`), `is_authenticated()` (set at `crates/smtp/src/inbound/data.rs:817-833`), `TaskCalendarItipContents` as `ItipMessages::new` fills it (`za_imip_mail` reuses `ItipMessages::new`) and `send_imip`'s `account_id_from_email(from)` rule with `must_match_sender` are taken from upstream as they stand.
- `compose_imip_message` is `send_imip`'s message builder: a change there flows into both paths (the fork skips `ItipTemplate::set_recipient`).
- `route`'s split by class relies on token authentication being the only reason for release 2's restrictions.
- D12's reuse of `za_key_rcpt` relies on a local session never calling `reset()` and on `data.rs:817` clearing it only after the DATA stage's last redaction read; at release 5 the flag is an equivalent mutant on that path (above).
- The drain's Exchange fill uses the account's first address (spec question 2), because the item does not carry the envelope recipient.
- `za_imip_uid_known` relies on the UID staying visible on a sealed pending item; for a UID the event index lacks it reads every notification row once and the content of the pending ones.
- The tracing and trace-module rules rely on upstream's SMTP session emitting exactly one DKIM signing-domain `Cache` event per accepted message (`data.rs:863-866`): a second lookup there fails `imip::traces` loudly.
- The tests' throttle handling relies on upstream's default "sender domain, recipient" inbound throttle (25 an hour); a module that mails one recipient often disables it or budgets.

## Final review

Pending: the whole-branch review runs after this note.
