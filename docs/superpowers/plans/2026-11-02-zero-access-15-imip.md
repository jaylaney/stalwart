# Zero-access plan 15: invitations by email (iMIP, release 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A key account receives invitations, updates, cancellations and replies from outside senders and plain local accounts as sealed mail-borne pending items that its drain applies with upstream's merge, and mails its own invitations, updates, cancellations and replies to outside and plain-local recipients through upstream's iMIP composer, handed to the SMTP queue in the sender's own request after its commit, never through the task queue.

**Architecture:**
- **Extractions** (Task 1, upstream files, no behaviour change, spec 3.1): `send_imip`'s setup, per-message step and per-recipient body become callable functions (sender lookup, RSVP links, redaction flag and session id as parameters); `itip_ingest`'s parse with the Exchange fill, its snapshot with the method's sender rule, and its auto-add predicate become free functions.
- **Seam** (Task 2, D11, D12): `common::za::imip` holds a `OnceLock` function pointer and `za_imip_submit`; `services::za_imip` implements it on the extracted composer with RSVP links off and `za_key_rcpt` set, registered in `spawn_services`. The za suite routes every non-local domain to a local SMTP sink.
- **Inbound** (Tasks 3-5, spec 4): the `X-ZA-SENDER` carrier and the generalized item writer; the key branch at delivery (`za_imip_item`), deciding from the message and keyless indexes; the drain's mail arm (`Incoming::Mail`, `Method::Add`, `Method::Refresh`), which repeats the checks and ends each item as upstream's ingest leaves the inbox.
- **Outbound** (Tasks 6-7, spec 5): `Recipient::Mail` with status `1.1`, routing by class, the fan-out returning mail attempts and an `internal_only` flag; every caller submits only after its commit returns `Ok`, with commit-failure hooks on each path.
- **Coverage** (Tasks 8-12): the other harnesses, the differential oracle (the stopping rule, spec 3.4), the trace and leak modules, the mutation sample, the manual checklist, `CLAUDE.md` and the outcome note.

The fork diff stays narrow: new modules (`crates/common/src/za/imip.rs`, `crates/services/src/za_imip.rs`, `crates/email/src/message/za/imip.rs`, `tests/src/za/imip/`, `tests/src/utils/smtp_sink.rs`) and edits in fork-owned files (`fanout.rs`, `drain.rs`, `seal/item.rs`, `test_hooks.rs`, `store/src/za_test.rs`, and `http/src/api/contacts.rs` for one fan-out argument). Upstream files change only as spec 10 lists: `imip.rs` (the extraction), `calendar/itip.rs` (the extraction), `services/src/lib.rs` (the registration line and `pub mod za_imip;`), `email/src/message/ingest.rs` (the gate, the call), `dav/src/calendar/{update,delete}.rs` (submission after the commit, the hooks), `common/src/za/mod.rs` (`pub mod imip;`) and `store/src/dispatch/store.rs` (one test-mode line).

**Tech Stack:** Rust 2024 (the fork on upstream Stalwart v1.0.1; internal crates `1.0.1`, `vault` `0.16.25`); mail-builder and mail-parser (upstream's composer), the smtp crate's local `Session::<NullIo>`, calcard (workspace crate), the seal helpers of `groupware::calendar::seal::tree` (XChaCha20-Poly1305 bundles); the `tests` crate harness (`TestServer`, `DummyWebDavClient`, `SmtpConnection` on LMTP 11200, `HookParked`, `DrainOn`, `uid_lease::put`, and a new SMTP sink).

**Spec:** `docs/superpowers/specs/2026-11-02-zero-access-imip-design.md`, revision 3, approved 2026-11-02 (`1b1cd094d`), cited by section number; where this plan and the spec disagree, the spec wins. Release 1, release 2, key pinning, mail and sharing specs bind where it cites them. Project rules: `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, the plan 3 outcome note (gate inventory), the 1.0.1 upgrade outcome note (hook inventory, conflict ledger).

**Spec questions (the plan's choice, pending Jay; none changes a decision of the spec):**
1. **8.5's inventory misses `fanout::test_plain_owner`** (`tests/src/za/fanout.rs:1272-1389`): key1 writes an invitation into plain's shared calendar, upstream's task mails it to key2, and from Task 4 on that mail and the DELETE's CANCEL become two mail-borne pending items in key2's inbox, which the test never deletes. Task 4 asserts them and deletes them.
2. **The drain's repeat of the Exchange fill (D5 step 1) has no recipient address**: delivery fills with the envelope recipient (`deliver_to`), which the item does not carry. The plan fills with the account's first address (`AccountInfo::addresses.first()`, no fill when the list is empty). On every item delivery wrote the repeat is a no-op, since delivery filled the tree before sealing it.
3. **The two-session REFRESH case (8.2, D14) names no hook** to hold the first session while the second drains. The plan adds one call to the existing `drain_lookup_pause_point` in the mail arm, after its UID lookup and before `merge`, instead of a new hook; `import` keeps its own call, so an import through the mail arm passes one pause point only.
4. **8.5's tracing case relies on "the submission's SMTP span id"**, but upstream's local session passes session id 0 (`imip.rs:255`). The per-recipient function takes the session id: `send_imip` passes 0 as today, and the fork passes a fresh id from its dummy instance's `span_id_gen`, so each submission is a span of its own that the trace tests can single out.
5. **3.1 item 1's "two functions"**: the extraction is a context type whose two methods are the task-level setup (`:82-126`) and the per-message step (`:127-171`, where the sender lookup runs), plus the per-recipient function (`:172-343`). Two more splits make 8.1's composer tests possible without a `Server`: `compose_imip_message` (the `MessageBuilder` expression, `:192-243`) and `ItipTemplate::from_template` (`new` without its template selection). None changes behaviour.
6. **8.2's "a mail-borne REQUEST merge that adds no delegate" on a copy holding pairs**: pairs reach a copy only from a key organizer, whose address 4.1 step 4 refuses as a mail sender, so no legitimate mail-borne REQUEST merges into such a copy. The plan's case delivers an outside party's REQUEST for a key-organized UID to the key attendee, whose copy holds its own pair; the merge is rejected (`OrganizerMismatch`), the item deleted, the pair unchanged. The group-organizer path of 5.1, the one route to a successful merge, is not exercised.
7. **8.5 misses `leak.rs`'s `SCHED_EVENT`** (`tests/src/za/leak.rs:214-221`): key2 invites `plain@example.com`, which from Task 7 on is mailed, so `sched-canary` reaches plain's mailbox, calendar and search index in the clear (exposure spec 2 accepts for a plain local recipient), where the whole-store canary scan reports it. The plan marks that ATTENDEE `SCHEDULE-AGENT=CLIENT`, so upstream's builder makes no message for it and the event still tests the organizer's sealed copy; the plain recipient's own copy is not scanned. Relatedly, 8.5 does not say that `plain@example.com` (a suite-wide account `assert_is_empty` keeps) must be cleared of what it now receives in `fanout::test` and `gating::test_scheduling`; Task 7 adds that cleanup.

**Execution:** subagent-driven development on a new branch `release5-imip` cut from `release5-imip-spec` (the spec approved at `1b1cd094d`, this plan committed on top). Implementers run at the tier each task names: `opus` for the kernels (the extractions, the seam's redaction and queue exclusion, the item format, the inbound path, the drain arm, outbound routing and ordering), `sonnet` for test-harness and verification work; reviewers at the session's ceiling (`opus`); no Fable review during execution (Jay authorizes it at the PR stage). Ledger: `.superpowers/sdd/2026-11-02-zero-access-15-imip/progress.md`.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build with `enterprise`.
- The http crate is `http@1.0.1` (`crates/http/Cargo.toml` version `1.0.1`): `cargo test -p http@1.0.1 --lib --bins --features test_mode,enterprise`; format it with `cargo fmt --manifest-path crates/http/Cargo.toml -- --check`.
- Per-task compile check, shipping configuration; the build must succeed (exit 0) before the warning filter runs, so a failed build is never read as `clean`:
  ```bash
  log="${TMPDIR:-/tmp}/za-check.log"
  cargo check -p stalwart --no-default-features --features rocks > "$log" 2>&1 || { tail -40 "$log"; echo BUILD FAILED; false; }
  grep -E "^(warning|error)" -A3 "$log" | grep -E "crates/(services|groupware|email|dav)/|crates/http/src/api/|crates/common/src/za/|crates/store/src/dispatch/store\.rs" || echo clean
  ```
  Upstream's known warnings elsewhere are not this plan's.
- Test-mode compile check: `cargo check -p tests --tests`, with no warning in `tests/src/za/imip/`, `tests/src/utils/smtp_sink.rs` or a line this plan touched.
- Unit tests: `cargo test -p groupware`, `cargo test -p email --features test_mode za::`, `cargo test -p common za::`, `cargo test -p store --lib --features test_mode za_test`, `cargo test -p services --lib za_imip`. If the services crate's unit build does not compile without features, use `cargo test -p services --lib --features test_mode,enterprise za_imip` (the unit-tests job's features for this crate) and say so in the task report.
- Integration: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`; at the end also with `BLOB_STORE=FileSystem`. CalDAV: the same prefix with `webdav::webdav_tests`, plain and with `ZA_KEY_ACCOUNTS=1`. Mail: `imap::imap_tests` and `jmap::jmap_tests`, plain and key mode. Upstream's `cal_itip` and `calendar::notification` sub-tests are timing-flaky: rerun once before calling a failure there a regression.
- One cargo command at a time; never while Jay's manual server is under test.
- Formatting: `cargo fmt -p <crate> -- --check` for `services`, `groupware`, `email`, `dav`, `common`, `store`, `tests`; the http form above.

Spec values, verbatim:
- `ITIP_STATUS_SENT = "1.1"` (RFC 6638 "sent, delivery not confirmed"), in `fanout.rs`, rank 0 in `status_rank`: below `5.3` (1), `5.1` (2) and `3.8` (3) (5.4).
- `X-ZA-SENDER` (`SENDER_PROP`): the sender's address sealed with the item's DEK by `seal_bytes` (format byte 1, padded to 256 bytes), associated data `za/v1|itip-sender|<recipient account id>|<UID>` (`aad("itip-sender", recipient_id)`, then `|`, then the UID). Root order: the `X-ZA-SEALED` entries, then exactly one of `X-ZA-RSVP-TOKEN`, `X-ZA-EXTRA` or `X-ZA-SENDER`, then `X-ZA-KEY`; anywhere else, or beside another of the three, it is a stray carrier. Only `za_imip_item` writes it; the drained form carries none of the three (4.2).
- `changed_by = ChangedBy::CalendarAddress(String::new())` for every mail-borne item, pending and drained (4.2, D4).
- Delivery's order (4.1): size, parse and Exchange fill, snapshot and the method's sender rule, the method (REQUEST, ADD, CANCEL, REPLY, REFRESH continue; others `UnsupportedMethod`), the key-account sender refusal, the known UID (event or pending item) with the auto-add predicate for a REQUEST, the item. A refusal writes nothing and never fails the delivery.
- The mail class is exactly "no local account, or a local account that is not a key account" (D9); not attempted and never mailed: a local key account without `CalendarSchedulingReceive` or a served public key, the sender's own account, the event owner on the delegated-organizer path (5.1).
- Every email is upstream's composition minus `ItipTemplate::set_recipient` (no RSVP token, no RSVP block); every session the outbound path opens has `SessionData::za_key_rcpt = true` set right after `Session::local` (D12); the sender lookup is the quiet `za_account_id_from_email` (D17).
- Mail is submitted only after the sender's commit returns `Ok` (Jay's decision 9); `1.1` is written before the commit; a failed submission is traced and not corrected.
- Fixed trace strings (section 7, D17): the spec's `zero-access: iMIP from a key account`; the plan's `zero-access: iMIP not taken`, `zero-access: inbox item refused`, `zero-access: iMIP delivery holds session keys`, `zero-access: iMIP item not written`, `zero-access: iMIP sender lookup failed`, `zero-access: mail-borne item deleted`, `zero-access: iMIP submission refused`, `zero-access: iMIP submission failed`, `zero-access: iMIP submitter not registered`, `zero-access: iMIP mail dropped, the deletion did not commit`; upstream's `iMIP message too large`. Every one lives as a constant in `common::za::imip`. `From` stays as a key with `trc::Value::redacted()`.
- Hooks (test mode only): `IMIP_SUBMIT_FAIL_ONCE`, `UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_FAIL_ONCE` in `common::za::test_hooks`; `SECOND_CHUNK_FAIL_ONCE` in `store::za_test`, its setter re-exported by `common::za::test_hooks`. Each is one-shot and keyed by account id.
- No setting, permission, event type or environment knob (1.3); no edit of `crates/registry/src/schema/*` or `crates/trc/src/event/enums.rs`.

Invariants and code rules:
- Every invariant 1-40 holds with spec 9's amendments; this release adds 41-47 and no invariant 9 exception. In short: an outbound message is composed in a request holding the sender's keys and handed to the SMTP queue in that request, never to the task queue or a store location first, with no fork carrier, token or RSVP link (41); only the mail class is mailed (42); a mail-borne item is written only at delivery, for a sender `is_authenticated()` accepts that is not a local key account, sealed to the served key, the sender only in `X-ZA-SENDER` (43); delivery decides from the message and keyless indexes, the drain repeats every check it can (44); the mail arm applies upstream's merge with the carrier's sender, without the stale-reply check, never by token, never adopting, replacing or removing a pair because of mail (45); a mailed recipient's status is `1.1`, written before the commit, its mail submitted only after the commit succeeds, never pinned (46); the fork's iMIP events carry no address or content and every outbound transaction is redacted (47).
- No stored struct changes layout: the sender rides in a root entry of the sealed item tree; `CalendarEventNotification`'s `changed_by` uses upstream's existing variant.
- Non-key accounts take unchanged upstream code paths: Task 1's extractions are called by upstream's code with upstream's arguments, and every new branch is a key account's.
- Background code never needs a key; the task queue never holds a key account's message. `ZaImipMail` and `PendingCarriers` have no `Debug` (content and addresses).
- Traces carry account ids, document ids, queue ids, counts, stage names, reply codes and fixed strings only. A store error from a lookup keyed by the sender's address is traced with its type and the account id only (section 7).
- Never open issues, pull requests or reports against upstream `stalwartlabs/stalwart`; an upstream bug found here goes in the outcome note only, described without naming the private tracker.
- Reviews (spec 3.4): once Task 9's oracle passes, a later finding is in scope only if it breaks invariants 41-47 (or an earlier one, as amended) or shows the oracle would pass on wrong behaviour; anything else is recorded as a residual or deferred. Findings about forged mail-borne items (Jay's decision 11) are out of scope; issue #72 owns them.
- Commits: the plan gives each subject; the executing session adds a blank line and its trailer lines.

Tests:
- New modules live in `tests/src/za/imip/` and run after `pinning::test` and before `sharing_move_atomic::test`, in the order `imip::inbound`, `imip::outbound`, `imip::oracle`, `imip::traces`; `leak::test_imip` runs after `leak::test_mail`. Each `pub async fn test(test: &mut TestServer)` holds `DrainOn` where it needs a drain (the suite sets `DRAIN_SKIP_ALL`, `tests/src/za/mod.rs:132`), triggers drains with `trigger_drain(name, STRONG)`, and deletes what it creates: events (with `Schedule-Reply: F` where a CANCEL must not go out), inbox items (`Party::delete_items_since`), contacts, mailing lists and its plain accounts (`admin.destroy_account`). Key fixtures are inserted with `test.insert_account` and destroyed with the suite's other key accounts (`destroy_key_accounts`).
- Fixtures, all of the `imip-*` family the spec reserves (8.2), unique across modules: `imip::inbound` creates `imip-key`, `imip-key2` (key) and `imip-plain` (plain); `imip::outbound` creates `imip-out-key`, `imip-out-key2`, `imip-alias` (alias `imip-alias-two`), `imip-grantee` (key), `imip-noperm` (key, without `CalendarSchedulingReceive`, as `fanout::test` removes it from key3), `imip-pending` (passwordless with a setup token, destroyed by the module), `imip-out-plain` (plain) and the mailing list `imip-list`; `imip::oracle` creates `imip-oracle-key` and `imip-oracle-plain`; `imip::traces` creates `imip-trace-key`; `leak::test_imip` creates `imip-leak-key` and `imip-leak-key2`; all at `example.com`. Outside parties are sink addresses, not accounts: `imip-org`, `imip-att`, `imip-delegate`, `imip-nodmarc`, `imip-stranger`, `imip-many-01` to `imip-many-20` and the leak canary addresses, all at `remote.test`. The mail modules' next free names (`keymail36`, `plainmail41`) do not move.
- Inbound mail goes over LMTP 11200 (`SmtpConnection::connect().ingest(envelope, &[to], message)`), with MAIL FROM `dmarc-<name>@remote.test` for the test-mode DMARC pass (`crates/smtp/src/inbound/data.rs:820-825`) and the From header naming the iTIP sender; `imip-nodmarc@remote.test` as MAIL FROM is unauthenticated. The junk case calls `email_ingest` with `IngestSource::Smtp { is_spam: true, .. }`, as `mail/fence.rs:1604-1625` calls it.
- From Task 7 on every write of a key organizer with an outside or plain-local attendee mails it, so a case that counts sink messages first calls `sink().clear()` and then counts only what its own step sent (by recipient, method and UID). `plain@example.com` outlives the suite: a module that mails it clears what it received before returning (`clear_received_scheduling`, Task 7).
- Outbound mail is read from the SMTP sink (`smtp_sink::sink()`) after `wait_for_queue`, which polls the SMTP queue only. `wait_for_delivery` also waits for tasks: use it only where upstream's task must run first (a plain account's scheduling), and never while an event with an `ACTION:EMAIL` alarm exists (its task is not yet due). Events in these modules carry `ACTION:DISPLAY` alarms only.
- The DAV test client times out at 500 ms (`tests/src/utils/webdav.rs:100`): a request that mails more than one recipient, runs under a pause, or drains REFRESH answers goes through `uid_lease::put`, `trigger_drain` or an `HttpRequest` with credentials (5 s).
- No test forges a mail-borne item in the store (Jay's decision 11; issue #72 owns its qualification test).
- New hooks are cleared at the end of the step that set them. An existing za assertion that fails only because a recipient is now mailed or an item is now written is adjusted with a comment citing the spec section and listed in the task report; any other failure is a regression.
- Kill checks and mutation runs are made against a commit, one at a time, each reverted with `git checkout -- <file>` and followed by an empty `git status --short crates/`; none is committed.

## Review Focus

Inputs the spec implies that no case of section 8 exercises, most likely to bite first. Each has its test in the owning task.

1. **An invitation as real providers compose it**: `multipart/alternative` with a base64-encoded `text/calendar; method=REQUEST` part plus an `application/ics` attachment of the same ICS. Expected: exactly one pending item, whose opened tree is the decoded calendar part. Task 4, `rf_1_provider_shaped_invitation`.
2. **The sender's address in another case or form**: From `"Org" <IMIP-ORG@Remote.Test>` with `ORGANIZER;CN=Org:mailto:imip-org@remote.test`. Expected as upstream: the item is written, its carrier holds the sanitized address, and the drain's repeated sender rule accepts it and imports the event. Task 5, `rf_2_sender_case`.
3. **A key attendee invited at an alias**: an outside REQUEST to `imip-alias-two@example.com`, accepted in the client. Expected: the REPLY goes out from the alias (one of the account's addresses, so `must_match_sender` passes) and reaches the sink. Task 7, `rf_3_alias_attendee`.
4. **A recurring series changed instance by instance from outside**: REQUEST for a weekly series, a REQUEST for one overridden instance, a CANCEL of another instance. Expected: the key account's copy equals the copy `imip-plain` stores for the same three emails. Task 5, `rf_4_recurring_instance`.
5. **A large outbound invitation**: twenty outside attendees. Expected: the PUT answers within 5 s, every attendee shows `1.1`, and the sink holds exactly twenty messages, one recipient each. Task 7, `rf_5_many_recipients`.

## Case map

| Spec item | Task |
|---|---|
| 3.1 extractions; 8.1 composer and RSVP-off units | 1 |
| 5.3 seam, D11, D12; the sink and catch-all route (8.2) | 2 |
| 4.2, D4; 8.1 seal units | 3 |
| 4.1, D1-D3, 4.5, 4.6 (delivery half), section 7 (delivery rows); 8.2 `imip::inbound` delivery cases; 8.5 `fanout.rs:1164-1270`, `gating.rs` (item), `mail/fence.rs:740-770` | 4 |
| 4.3, 4.4, D5-D7, D14, D15's gate, 4.5's plain local organizer; 8.2 `imip::inbound` drain cases and `imip::outbound`'s two-session REFRESH case; 8.5 `gating.rs` (copy after a drain) | 5 |
| 5.1, 5.2, D9, D10, decision 10, D15's flag, 5.4 status; 8.1 `route` and rank units; 8.5 `fanout.rs:271,275,340,994`, `sharing.rs:7721-7729`, `tracing.rs` statuses | 6 |
| 5.4 order, 5.5 callers, decision 9, the commit hooks, `SECOND_CHUNK_FAIL_ONCE`, 4.5's plain local attendee (it needs the outbound path); 8.2 `imip::outbound`; 8.5 mail at the sink and at `plain` (`fanout.rs`, `gating.rs`, `tracing.rs:871-895`, `leak.rs`, `pinning.rs:1212`, `sharing.rs:5673`) | 7 |
| 8.2's other harnesses (key-mode `webdav_tests`, `imap_tests`, `jmap_tests`, the CalDAV client's key mode, the fresh store) | 8 |
| 3.3, 8.3 oracle | 9 |
| Section 7, D17, invariant 47; 8.2 `imip::traces` | 10 |
| 8.4 `leak::test_imip` | 11 |
| 8.6 mutations, 8.7 checklist and item 13, section 9 and 10 in `CLAUDE.md`, the outcome note | 12 |

Invariants: 41 is tested in Tasks 2, 7 and 11; 42 in 6; 43 in 3, 4 and 11; 44 in 4 and 5; 45 in 5; 46 in 6 and 7; 47 in 2, 10 and 11. Spec 9's amendments and section 10's inventories go into `CLAUDE.md` in Task 12.

---

### Task 1: Upstream extractions, no behaviour change

Implementer: `opus` (a kernel: the composer and the ingest checks must stay byte- and order-identical for every ordinary account).

**Files:**
- Modify: `crates/services/src/task_manager/tasks/imip.rs` (`send_imip`, `:76-369`, becomes calls to the functions below; `ItipTemplate::new`, `:411-679`, keeps its signature and delegates after its template selection, `:419-431`)
- Modify: `crates/groupware/src/calendar/itip.rs` (`itip_ingest`'s `:116-183` and `:266-283` become calls; the new functions after the `impl ItipIngest for Server` block)
- Create: `crates/services/src/za_imip.rs` (this task: the SPDX header, a module doc citing spec 3.1 and 5.3, and `mod tests`)
- Modify: `crates/services/src/lib.rs:19-22` (`pub mod za_imip;`)
- Test: `crates/services/src/za_imip.rs` (`mod tests`), `crates/groupware/src/calendar/drain.rs` (`mod tests`, `:1882`)

**Interfaces:**
- Consumes: upstream only.
- Produces, in `services::task_manager::tasks::imip` (all `pub(crate)` unless noted):
  - `struct ImipContext { logo: MimePart<'static>, logo_cid: String, account_info: AccountInfo, formatter: TextFormatter, account_id: u32, document_id: u32 }` and `async fn imip_context(server: &Server, account_id: u32, document_id: u32, sender_domain: &str) -> trc::Result<ImipContext>` (`:82-126`: the logo with its error trace, `logo.<now>@<domain>`, the account info, the formatter).
  - `#[derive(Clone, Copy, PartialEq, Eq)] enum SenderLookup { Traced, Quiet }`: `Traced` is `account_id_from_email(from, true)`, `Quiet` is `za_account_id_from_email(from)`.
  - `struct ImipComposer<'x>` (the template, the sender's `AccountInfo`, the message, the shared body) and `impl ImipContext { async fn composer<'x>(&'x self, server: &'x Server, message: &'x TaskCalendarItipContents, lookup: SenderLookup) -> Option<ImipComposer<'x>> }` (`:127-171`; `None` when the summary does not parse; the two error traces of the sender lookup stay as they are).
  - `struct ImipSubmitOptions { rsvp: bool, za_redact: bool, session_id: u64 }`; `struct ImipRejection { stage: &'static str, response: String }` with `fn into_smtp_rejection(self) -> SmtpRejection` (`SmtpRejection::new(stage, &response)`).
  - `impl ImipComposer<'_> { async fn submit(&mut self, server: &Server, context: &ImipContext, instance: Arc<ServerInstance>, recipient: &str, options: ImipSubmitOptions) -> trc::Result<Result<u64, ImipRejection>> }` (`:172-343` without its three `trc::event!` calls): `set_recipient` runs only with `options.rsvp`; `SessionData::local(sender_info, None, vec![], vec![], options.session_id)`; `session.data.za_key_rcpt = options.za_redact` on the line after `Session::local`; MAIL FROM, RCPT TO (`rcpt_errors_wait` zero), `queue_message(MessageOrigin::Internal)`; `Ok(queue_id)` from `State::Accepted`; the spawned task's `JoinError` is upstream's `ServerEvent::ThreadError`.
  - `fn compose_imip_message(sender_name: &str, from: &str, to: &str, message_id: String, subject: &str, body: &RenderedBody, method: &str, i_calendar_data: &str, logo: MimePart<'static>) -> Vec<u8>` (`:192-243` verbatim, `message_id` passed in).
  - `pub fn ItipTemplate::from_template(template: &'x Template<CalendarTemplateVariable>, formatter: &TextFormatter, from: &str, summary: &ItipSummary, logo_cid: &str) -> Self`.
  - `send_imip` keeps its signature and outcome: `imip_context`, then per message `composer(.., SenderLookup::Traced)` (`None` is `Outcome::fatal("Failed to parse iMIP message summary.")`), then per recipient `submit(.., ImipSubmitOptions { rsvp: true, za_redact: false, session_id: 0 })`, and from each result exactly today's event: a rejection's `Calendar(ItipMessageError)` with `AccountId`, `DocumentId`, `From`, `To`, `Reason = SmtpRejection.reason`; an acceptance's `Calendar(ItipMessageSent)` with `From`, `To`, `AccountId`, `DocumentId`, `QueueId`. A temporary rejection still joins `retry_to`.
- Produces, in `groupware::calendar::itip` (free `pub` functions):
  - `fn itip_parse_inbound(text: &str, recipient: &str) -> Result<ICalendar, ItipError>` (`:117-127`, then `itip_fill_organizer`).
  - `fn itip_fill_organizer(itip: &mut ICalendar, recipient: &str)` (`:129-163`: REPLY and REQUEST only; an ORGANIZER only where a scheduling component has ATTENDEEs and none).
  - `fn itip_sender_snapshot<'x>(itip: &'x ICalendar, addresses: &[String], sender: &str) -> Result<ItipSnapshots<'x>, ItipError>` (`:165-183`).
  - `async fn itip_auto_add_allowed(server: &Server, account_id: u32, sender_is_principal: bool, sender: &str) -> trc::Result<bool>` (`:267-278`: `itip_auto_add || sender_is_principal || document_exists(account_id, ContactCard, ContactField::Email, sender)`, evaluated in that order).
  - `itip_ingest` calls them with `account_info.addresses()`, `matches!(changed_by, ChangedBy::PrincipalId(_))` and its own arguments; its `changed_by`, lookup, merge, import and error mapping are unchanged.

- [ ] **Step 1: Write the failing composer tests in `crates/services/src/za_imip.rs`**

```rust
/// Spec 8.1: the extracted composer writes the bytes v1.0.1's `send_imip`
/// wrote. `upstream_builder` is the tag's `imip.rs:192-243` expression copied
/// verbatim into the test; `normalize` replaces MIME boundaries and the Date
/// header by placeholders in both; Message-ID and the logo CID are pinned.
#[test]
fn the_extracted_composer_writes_the_tasks_bytes() {
    let body = RenderedBody { html: "<p>Sync</p>".into(), text: "Sync".into() };
    let logo = || default_logo("logo.1@remote.test"); // upstream's DEFAULT_LOGO_BASE64 part
    let ours = compose_imip_message("Org", "imip-org@remote.test", "imip-att@remote.test",
        "pinned@example.com".into(), "Invitation: Sync", &body, "REQUEST", ICS, logo());
    let upstream = upstream_builder("Org", "imip-org@remote.test", "imip-att@remote.test",
        "pinned@example.com".into(), "Invitation: Sync", &body, "REQUEST", ICS, logo());
    assert_eq!(normalize(&ours), normalize(&upstream));
    let text = String::from_utf8_lossy(&ours);
    assert!(text.contains("method=REQUEST") && text.contains("event.ics"));
    assert!(text.contains("Auto-Submitted: auto-generated"));
}

/// Spec 5.3, 8.1: a body rendered without `set_recipient` has no RSVP block.
#[test]
fn a_template_without_rsvp_renders_no_rsvp_block() {
    let template = Template::parse(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
        "/../../resources/html-templates/calendar-invite.html.min"))).unwrap();
    let formatter = TextFormatter::new("en").unwrap();
    let summary = ItipSummary::Invite(vec![summary_field("Sync")]);
    let rendered = ItipTemplate::from_template(&template, &formatter, "imip-org@remote.test",
        &summary, "logo.1@remote.test").render();
    assert!(rendered.html.contains("Sync"), "the template rendered");
    let reply_as = formatter.locale.calendar_reply_as.replace("$name", "imip-att@remote.test");
    assert!(!rendered.html.contains(&reply_as) && !rendered.html.contains("?i="));
}
```

- [ ] **Step 2: Write the failing extraction tests in `drain.rs`'s `mod tests`**

```rust
#[test]
fn the_exchange_fill_adds_an_organizer_only_where_missing() {
    let mut reply = parse(REPLY_WITHOUT_ORGANIZER); // ATTENDEE imip-att, no ORGANIZER
    itip_fill_organizer(&mut reply, "imip-key@example.com");
    assert_eq!(organizers(&reply), ["mailto:imip-key@example.com"]);
    itip_fill_organizer(&mut reply, "other@example.com");
    assert_eq!(organizers(&reply), ["mailto:imip-key@example.com"], "idempotent");
    let mut cancel = parse(CANCEL_WITHOUT_ORGANIZER);
    itip_fill_organizer(&mut cancel, "imip-key@example.com");
    assert!(organizers(&cancel).is_empty(), "REPLY and REQUEST only");
    assert!(matches!(itip_parse_inbound("not ical", "x@example.com"), Err(ItipError::ICalendarParseError)));
}

#[test]
fn the_sender_rule_follows_the_method() {
    let own = vec!["imip-key@example.com".to_string()];
    let request = parse(REQUEST_FROM_ORG); // ORGANIZER imip-org, ATTENDEE imip-key
    assert!(itip_sender_snapshot(&request, &own, "imip-org@remote.test").is_ok());
    assert!(matches!(itip_sender_snapshot(&request, &own, "imip-att@remote.test"),
        Err(ItipError::SenderIsNotOrganizer)));
    let reply = parse(REPLY_FROM_ATT); // ORGANIZER imip-key, ATTENDEE imip-att
    assert!(itip_sender_snapshot(&reply, &own, "imip-att@remote.test").is_ok());
    assert!(itip_sender_snapshot(&reply, &own, "imip-key@example.com").is_ok(), "3.2 item 12");
    assert!(matches!(itip_sender_snapshot(&reply, &own, "imip-stranger@remote.test"),
        Err(ItipError::SenderIsNotOrganizerNorAttendee)));
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p services --lib za_imip`, then `cargo test -p groupware drain::tests`.
Expected: compile errors naming `compose_imip_message`, `ItipTemplate::from_template`, `itip_fill_organizer`, `itip_sender_snapshot`.

- [ ] **Step 4: Split `send_imip` and `ItipTemplate::new` into the Interfaces block's functions**

`send_imip`'s body becomes the loop over messages and recipients, the three events and the outcome; every other line moves into the functions unchanged. The fork's module calls nothing yet.

- [ ] **Step 5: Split `itip_ingest`'s start and its auto-add predicate into the Interfaces block's functions**

`itip_ingest` maps each `ItipError` into `ItipIngestError::Message` as today (`?` through the existing `From` impl, `itip.rs:1545-1549`).

- [ ] **Step 6: Run the unit tests and checks**

Run: `cargo test -p services --lib za_imip`; `cargo test -p groupware`; the formatting check; the per-task compile check; `cargo check -p tests --tests`.
Expected: pass; no formatting difference; `clean`; clean.

- [ ] **Step 7: Run the suites that exercise upstream's iMIP paths**

Run, one at a time: `webdav::webdav_tests` (plain: `cal_scheduling` mails through the task, `send_imip` and local delivery into `itip_ingest`), `jmap::jmap_tests` (plain: JMAP scheduling and iMIP), `za::za_tests`.
Expected: all pass unchanged.

- [ ] **Step 8: Commit**

```bash
git add crates/services/src/task_manager/tasks/imip.rs crates/services/src/za_imip.rs crates/services/src/lib.rs crates/groupware/src/calendar/itip.rs crates/groupware/src/calendar/drain.rs
git commit -m "Release 5: split send_imip's composer and itip_ingest's checks into callable functions"
```

### Task 2: The submission seam and the suite's SMTP sink

Implementer: `opus` (a confidentiality kernel: the redaction flag and the exclusion of the task queue; the sink is harness work done in the same pass).

**Files:**
- Create: `crates/common/src/za/imip.rs`
- Modify: `crates/common/src/za/mod.rs:13-18` (`pub mod imip;`; the module doc gains release 5)
- Modify: `crates/common/src/za/test_hooks.rs` (`IMIP_SUBMIT_FAIL_ONCE` beside `COMMIT_FAIL_ONCE`, `:148`; its setter beside `:229`; its `take_` beside `:468`; the module doc's hook list)
- Modify: `crates/services/src/za_imip.rs` (the submitter)
- Modify: `crates/services/src/lib.rs:48-69` (first line inside the `if` block: `za_imip::register();`)
- Create: `tests/src/utils/smtp_sink.rs`; Modify: `tests/src/utils/mod.rs` (`pub mod smtp_sink;`)
- Modify: `tests/src/utils/za.rs` (`wait_for_queue`, beside `wait_for_delivery`, `:996`)
- Create: `tests/src/za/imip/mod.rs`, `tests/src/za/imip/outbound.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod imip;` in `:16-49`; `imip::route_outside_mail(&admin, &test.server).await;` before `admin.reload_settings()` at `:126`; `imip::outbound::test(&mut test).await;` after `pinning::test` at `:170`)

**Interfaces:**
- Consumes: Task 1 (`imip_context`, `ImipContext::composer`, `ImipComposer::submit`, `ImipSubmitOptions`, `ImipRejection`, `SenderLookup::Quiet`).
- Produces, in `common::za::imip`:
  - `pub struct ZaImipMail { pub account_id: u32, pub document_id: u32, pub contents: TaskCalendarItipContents }` (no `Debug`): the event's account (upstream's task account), the event's document id, and one message as `ItipMessages::new` fills it with `to` the one recipient (5.3).
  - `pub enum ZaImipOutcome { Queued(u64), Refused { stage: &'static str, code: u16 }, Failed }`.
  - `pub type ZaImipSubmitter = fn(Server, ZaImipMail) -> Pin<Box<dyn Future<Output = ZaImipOutcome> + Send>>;` and `pub fn za_register_imip_submitter(submitter: ZaImipSubmitter)` (a `static OnceLock`; a second registration is ignored).
  - `pub async fn za_imip_submit(server: &Server, mail: ZaImipMail) -> ZaImipOutcome`: with `IMIP_SUBMIT_FAIL_ONCE` taken for `mail.account_id`, or no submitter registered, `Failed` without calling it; it traces every outcome (section 7): `Queued` as `Calendar(ItipMessageSent)` with `AccountId`, `DocumentId`, `QueueId`; `Refused` as `Calendar(ItipMessageError)` with `AccountId`, `DocumentId`, `Reason = stage`, `Code = code`, `Details = ZA_IMIP_REFUSED`; `Failed` with `Details = ZA_IMIP_FAILED`, or `ZA_IMIP_UNREGISTERED` before registration.
  - `pub async fn za_imip_submit_all(server: &Server, mails: Vec<ZaImipMail>)`: each in order.
  - The constants `ZA_IMIP_REFUSED`, `ZA_IMIP_FAILED`, `ZA_IMIP_UNREGISTERED` (Global Constraints strings).
- Produces, in `common::za::test_hooks`: `pub static IMIP_SUBMIT_FAIL_ONCE: Mutex<Option<u32>>`, `pub fn set_imip_submit_fail_once(account_id: Option<u32>)`, `pub fn take_imip_submit_fail(account_id: u32) -> bool`.
- Produces, in `services::za_imip`: `pub fn register()` (registers `submit`); `fn submit(server: Server, mail: ZaImipMail) -> Pin<Box<..>>`, which builds `imip_context(&server, mail.account_id, mail.document_id, <domain of contents.from, else "localhost">)`, the composer with `SenderLookup::Quiet`, and calls `submit(.., ImipSubmitOptions { rsvp: false, za_redact: true, session_id: instance.span_id_gen.generate() })` on a process-wide dummy instance (`OnceLock<Arc<ServerInstance>>`, built as `crates/services/src/task_manager/manager.rs:61-70` builds the task manager's: id `_local`, SMTP, plain acceptor); a context or composer error is `trc::error!` with the account id and `ZA_IMIP_FAILED`, then `Failed`; `pub(crate) fn reply_code(response: &str) -> u16` (the leading three digits, else 0).
- Produces, in `tests/src/utils/smtp_sink.rs`: `pub const ZA_SINK_PORT: u16 = 12525;` (unused elsewhere in `tests/src`); `pub struct SinkMessage { pub mail_from: String, pub rcpt_to: Vec<String>, pub data: Vec<u8> }` with `parsed()`, `header(name) -> Option<String>`, `from_address() -> Option<String>`, `calendar_part() -> Option<String>` (the decoded `text/calendar` part), `calendar_method() -> Option<String>`, `text_part()`, `html_part()`, `decoded_parts() -> Vec<Vec<u8>>` (every part, base64 and quoted-printable decoded); `pub struct SmtpSink` with `messages()`, `take_to(rcpt) -> Vec<SinkMessage>` (removes them; case-insensitive), `count_to(rcpt) -> usize`, `clear()`; `pub async fn start(port: u16) -> Arc<SmtpSink>`; `pub fn sink() -> Arc<SmtpSink>` (the suite's; panics before `start`). The server greets `220`, answers EHLO/HELO with `250` and no extension (no STARTTLS, PIPELINING or CHUNKING), MAIL, RCPT, RSET and NOOP with `250`, DATA with `354` then `250` after the dot (dot-unstuffed), QUIT with `221`; one task per connection.
- Produces, in `tests/src/utils/za.rs`: `pub async fn wait_for_queue(test: &TestServer)` (`wait_for_delivery`'s SMTP-queue poll alone, its 10 s deadline included).
- Produces, in `tests/src/za/imip/mod.rs`: `pub const ORG: &str = "imip-org@remote.test";` and `ATT`, `DELEGATE`, `NODMARC`, `STRANGER` alike; `pub async fn route_outside_mail(admin: &Account, server: &Server)`: starts the sink, adds `za-sink.test` → 127.0.0.1 to the DNS cache for 24 h (`DnsCache::ipv4_add`), creates `MtaRoute::Relay(MtaRouteRelay { name: "za-sink", host: "za-sink.test", port: ZA_SINK_PORT, protocol: MtaProtocol::Smtp, implicit_tls: false, allow_invalid_certs: true, .. })` and updates the outbound strategy's route only (`registry_update_setting(MtaOutboundStrategy { route: <is_local_domain(rcpt_domain) → 'local', else 'za-sink'>, ..Default::default() }, &[Property::Route])`); the reload at `:126` follows. `MtaRoute` is a kept fixture type of `assert_is_empty` (`tests/src/utils/cleanup.rs:489-500`), and `mail::delivery::unconfigure` resets `Property::Schedule` only. If the default TLS strategy refuses plaintext, the route's strategy gets a `MtaTlsStrategy { name: "za-sink-tls", start_tls: Optional, .. }`; say so in the report.
- Produces, in `tests/src/za/imip/outbound.rs`: `pub async fn test(test: &mut TestServer)`, creating `imip-out-key` in this task.

- [ ] **Step 1: Write the failing unit test in `za_imip.rs`**

```rust
#[test]
fn reply_codes_are_read_from_the_response() {
    assert_eq!(reply_code("550 5.1.2 Mailbox does not exist."), 550);
    assert_eq!(reply_code("451 4.3.0 try later"), 451);
    assert_eq!((reply_code(""), reply_code("abc")), (0, 0));
}
```

- [ ] **Step 2: Write the failing seam case in `imip/outbound.rs`**

```rust
/// Spec 5.3, D11, D12: one (message, recipient) in process memory, handed to
/// the SMTP queue: upstream's email without RSVP links.
async fn seam(test: &TestServer, owner: &Party) {
    let mail = |to: &str| ZaImipMail { account_id: owner.id, document_id: 0,
        contents: TaskCalendarItipContents { from: owner.name.into(), to: Map::new(vec![to.into()]),
            is_from_organizer: true, i_calendar_data: SEAM_ICS.into(),
            summary: serde_json::to_string(&ItipSummary::Invite(vec![])).unwrap() } };
    assert!(matches!(za_imip_submit(&test.server, mail(ATT)).await, ZaImipOutcome::Queued(_)));
    wait_for_queue(test).await;
    let got = sink().take_to(ATT);
    assert_eq!(got.len(), 1, "one email, one recipient");
    assert_eq!((got[0].mail_from.as_str(), got[0].rcpt_to.as_slice()), (owner.name, [ATT.to_string()].as_slice()));
    assert_eq!(got[0].header("Auto-Submitted").as_deref(), Some("auto-generated"));
    assert_eq!(got[0].calendar_method().as_deref(), Some("REQUEST"));
    assert_eq!(got[0].calendar_part().unwrap().trim_end(), SEAM_ICS.trim_end());
    assert!(!got[0].html_part().unwrap().contains("/calendar/rsvp"), "no RSVP link");
    // A local recipient the SMTP layer refuses at RCPT: nothing queued.
    assert!(matches!(za_imip_submit(&test.server, mail("imip-nobody@example.com")).await,
        ZaImipOutcome::Refused { stage: "RCPT-TO", code } if code / 100 == 5));
    assert!(queued_recipients(test).await.is_empty());
    // The hook: traced, nothing sent.
    test_hooks::set_imip_submit_fail_once(Some(owner.id));
    assert!(matches!(za_imip_submit(&test.server, mail(ATT)).await, ZaImipOutcome::Failed));
    wait_for_queue(test).await;
    assert!(sink().take_to(ATT).is_empty());
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p services --lib za_imip`; `cargo check -p tests --tests`.
Expected: compile errors naming `reply_code`, `ZaImipMail`, `za_imip_submit`, `smtp_sink`.

- [ ] **Step 4: Implement the seam (common, services) and the registration line**

- [ ] **Step 5: Implement the sink, `wait_for_queue`, `route_outside_mail` and the module wiring**

- [ ] **Step 6: Run the tests and checks**

Run: `cargo test -p services --lib za_imip`; `cargo test -p common za::`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite.
Expected: pass; pass; no difference; `clean`; clean; the za suite passes with `imip::outbound`'s seam case (the catch-all route changes no existing module: nothing in it mails a non-local domain yet).

- [ ] **Step 7: Commit**

```bash
git add crates/common/src/za/imip.rs crates/common/src/za/mod.rs crates/common/src/za/test_hooks.rs crates/services/src/za_imip.rs crates/services/src/lib.rs tests/src/utils/smtp_sink.rs tests/src/utils/mod.rs tests/src/utils/za.rs tests/src/za/imip tests/src/za/mod.rs
git commit -m "Release 5: the iMIP submission seam and the suite's SMTP sink"
```

### Task 3: The `X-ZA-SENDER` carrier and the generalized item writer

Implementer: `opus` (a kernel: the sealed item format, its stray-carrier rule and the writer every pending item goes through).

**Files:**
- Modify: `crates/groupware/src/calendar/seal/item.rs` (module doc `:7-23`; constants after `:62`; `PendingCarriers` `:64-70`; `seal_pending_item` `:219-252`; `open_pending_item` `:254-309`; `mod tests`)
- Modify: `crates/groupware/src/calendar/seal/mod.rs:26-27` (re-export `SENDER_PROP`)
- Modify: `crates/groupware/src/calendar/fanout.rs:1437-1589` (`write_item` keeps its signature and calls `za_write_pending_item`; `write_item_inner` becomes it)
- Modify: `crates/groupware/src/calendar/drain.rs:1830` (the doc reference to `write_item_inner`)
- Modify: `tests/src/utils/za.rs:1607` (`plant_inbox_item` passes `None` for the sender)

**Interfaces:**
- Consumes: `seal_bytes`, `open_bytes` (`seal/tree.rs:259`, `:307`), `vault::keys::aad`.
- Produces, in `groupware::calendar::seal::item`:
  - `pub const SENDER_PROP: &str = "X-ZA-SENDER";`; `const AAD_ITIP_SENDER: &str = "itip-sender";` and `fn sender_aad(recipient_id: u32, uid: &str) -> Vec<u8>` in `extra_aad`'s shape.
  - `PendingCarriers` gains `pub sender: Option<String>` (still no `Debug`).
  - `pub fn seal_pending_item(ical: &mut ICalendar, recipient_id: u32, recipient_public: &[u8; PUBLIC_KEY_LEN], pair: Option<&TokenPairs>, reply_token: Option<&[u8; TOKEN_LEN]>, sender: Option<&str>) -> Result<(), SealError>`: at most one carrier (`Structure("a pair and a reply token")` as today; `Structure("a sender and another carrier")` for a sender beside either); the sender's entry is `text_entry(SENDER_PROP, seal_bytes(&dek, &sender_aad(..), sender.as_bytes()))`, pushed where `X-ZA-EXTRA` is, before `X-ZA-KEY`.
  - `open_pending_item` pops `X-ZA-EXTRA`, else `X-ZA-RSVP-TOKEN`, else `X-ZA-SENDER`, before its stray check (so a second carrier, or one out of place, is `Structure("misplaced carrier")`); the sender opens with `open_bytes` and must be UTF-8 (`SealError::Format`).
- Produces, in `groupware::calendar::fanout`: `pub async fn za_write_pending_item(server: &Server, changed_by: ChangedBy, recipient_id: u32, public_key: &[u8; 32], message: &ICalendar, pair: Option<&TokenPairs>, reply_token: Option<&[u8; TOKEN_LEN]>, sender: Option<&str>, fence: Option<&ZaUidLease>) -> trc::Result<bool>`: `write_item_inner`'s steps unchanged (inbound size check, notification limit with the eviction of the oldest drained item, `za_item_shape`, the seal with the carrier, the quota on the sealed size, the fan-out pause point, one batch), the row built by `sealed_item_record(tree, changed_by, None, true)`. `write_item` passes `ChangedBy::PrincipalId(sender_id)` and `None`.

- [ ] **Step 1: Write the failing seal tests in `item.rs`'s `mod tests`**

```rust
#[test]
fn a_sender_carrier_round_trips() {
    let (public, private) = generate_keypair();
    let mut ical = parse(ITEM);
    seal_pending_item(&mut ical, 9, &public, None, None, Some("imip-org@remote.test")).unwrap();
    assert!(!ical.to_string().contains("imip-org"), "the address is sealed");
    let carriers = open_pending_item(&mut ical, 9, &private).unwrap();
    assert_eq!(carriers.sender.as_deref(), Some("imip-org@remote.test"));
    assert!(carriers.pair.is_none() && carriers.reply_token.is_none());
}

#[test]
fn the_sender_carrier_binds_recipient_uid_and_dek() {
    // The `X-ZA-SENDER` entry of item A moved into item B: B for another
    // recipient (same UID), B with another UID (same recipient), and B with
    // the same recipient and UID but its own DEK.
    for (recipient, uid) in [(10, "item-1"), (9, "item-2"), (9, "item-1")] {
        let moved = transplant_sender(&sealed_with_sender(9, "item-1"), recipient, uid);
        assert!(open_pending_item(&mut moved.tree, recipient, &moved.private).is_err(), "{recipient} {uid}");
    }
}

#[test]
fn a_sender_beside_another_carrier_or_out_of_place_is_stray() {
    for root in [&["X-ZA-EXTRA", "X-ZA-SENDER"][..], &["X-ZA-SENDER", "X-ZA-EXTRA"],
                 &["X-ZA-RSVP-TOKEN", "X-ZA-SENDER"], &["X-ZA-SENDER", "X-ZA-RSVP-TOKEN"]] {
        assert_eq!(open_pending_item(&mut hand_built(root), 9, &private()).err(),
            Some(SealError::Structure("misplaced carrier")), "{root:?}");
    }
    assert_eq!(open_pending_item(&mut sender_in_vevent(), 9, &private()).err(),
        Some(SealError::Structure("misplaced carrier")));
    assert_eq!(seal_pending_item(&mut parse(ITEM), 9, &public(), Some(&pair()), None, Some("a@b.test")),
        Err(SealError::Structure("a sender and another carrier")));
}

#[test]
fn the_drained_form_carries_no_sender() {
    let mut ical = sealed_with_sender(9, "item-1").tree;
    open_pending_item(&mut ical, 9, &private()).unwrap();
    seal_drained_item(&mut ical, &keys(9), 9).unwrap();
    assert!(!ical.to_string().contains(SENDER_PROP));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p groupware seal::item`
Expected: compile errors naming `SENDER_PROP` and the sixth argument of `seal_pending_item`.

- [ ] **Step 3: Implement the carrier and `za_write_pending_item`; update the two other callers**

- [ ] **Step 4: Run the tests and checks**

Run: `cargo test -p groupware`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite.
Expected: pass; no difference; `clean`; clean; the za suite passes unchanged (fan-out items still carry `PrincipalId` and no sender).

- [ ] **Step 5: Commit**

```bash
git add crates/groupware/src/calendar/seal/item.rs crates/groupware/src/calendar/seal/mod.rs crates/groupware/src/calendar/fanout.rs crates/groupware/src/calendar/drain.rs tests/src/utils/za.rs
git commit -m "Release 5: the X-ZA-SENDER carrier and the item writer generalized over the sender"
```

### Task 4: Inbound at delivery

Implementer: `opus` (a kernel: the decision to write a sealed item from a message and keyless indexes, with no session key, and the trace floor).

**Files:**
- Modify: `crates/email/src/message/ingest.rs:505-513` (the gate loses `&& !za_key`), `:523-524` (for a key account, the call and `continue`, ahead of upstream's size check)
- Modify: `crates/email/src/message/za/mod.rs` (`pub mod imip;`)
- Create: `crates/email/src/message/za/imip.rs`
- Modify: `crates/common/src/za/imip.rs` (the delivery constants)
- Create: `tests/src/za/imip/inbound.rs`; Modify: `tests/src/za/imip/mod.rs` (helpers), `tests/src/utils/za.rs` (`mail_borne_items`), `tests/src/za/mod.rs` (`imip::inbound::test` after `pinning::test`, before `imip::outbound::test`)
- Modify: `tests/src/za/fanout.rs:1164-1263` (`test_task_queue`), `:1272-1389` (`test_plain_owner`, spec question 1), `tests/src/za/gating.rs:651-674`, `tests/src/za/mail/fence.rs:740-770`

**Interfaces:**
- Consumes: Task 1 (`itip_parse_inbound`, `itip_sender_snapshot`, `itip_auto_add_allowed`); Task 3 (`za_write_pending_item`); `IngestWrap` (`za/ingest.rs:37-43`); `itip_reason` (`drain.rs:1847`); `Server::za_account_id_from_email`, `za_is_key_account`.
- Produces, in `email::message::za::imip`:
  - `pub fn za_imip_sender(sender_authentication: SenderAuthentication, envelope_from: &str, message: &Message<'_>) -> Option<String>`: upstream's rule (`ingest.rs:527-536`), the envelope MAIL FROM for `Submission`, else the first From address, sanitized.
  - `pub async fn za_imip_uid_known(server: &Server, account_id: u32, uid: &str) -> trc::Result<bool>` (D3): the UID index (`document_ids_matching(CalendarEvent, CalendarEventField::Uid)`) has a hit, or a pending row (`archived_item_is_pending`) of `Collection::CalendarEventNotification` whose content tree (`item_message`) names the UID (`uids()`), read through `server.archives` and the content field.
  - `#[allow(clippy::too_many_arguments)] pub async fn za_imip_item(server: &Server, account: &Arc<AccountCache>, wrap: Option<&IngestWrap>, itip_text: &str, sender_authentication: SenderAuthentication, envelope_from: &str, message: &Message<'_>, recipient: &str, session_id: u64)`: 4.1 in order; returns nothing, traces everything (below), never fails the delivery. The item's batch carries no provisioning fence and `za_has_tasks` is unchanged (D16).
- Produces, in `common::za::imip`: `ZA_IMIP_NOT_TAKEN`, `ZA_IMIP_KEY_SENDER` (`zero-access: iMIP from a key account`), `ZA_IMIP_ITEM_REFUSED`, `ZA_IMIP_SESSION_WRAP`, `ZA_IMIP_WRITE_FAILED`, `ZA_IMIP_LOOKUP_FAILED`.
- Produces, in `tests/src/za/imip/mod.rs`: `pub fn event(uid: &str, organizer: &str, attendees: &[&str], sequence: u32, extra: &str) -> String` (a VCALENDAR with one VEVENT starting in 2099, no METHOD, each attendee `ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE`, `extra` lines inserted before `END:VEVENT`); `pub fn with_method(ics: &str, method: &str) -> String`; `pub fn itip_mail(from_header: &str, to: &str, method: &str, ics: &str) -> String` (`multipart/mixed`: `text/plain`, then `text/calendar; method=<M>; charset=utf-8`); `pub async fn deliver(envelope_from: &str, to: &str, message: &str)` (LMTP 11200); `pub async fn add_contact(client: &DummyWebDavClient, account: &str, email: &str) -> String` (a CardDAV PUT into the account's default address book, the href) and `pub async fn delete_contact(client, href)`.
- Produces, in `tests/src/utils/za.rs`: `pub async fn mail_borne_items(test: &TestServer, account_id: u32) -> Vec<(u32, InboxItem)>` (rows whose `changed_by` is `CalendarAddress("")`).

`za_imip_item`, which the signature does not determine (4.1, section 7; every event carries `SpanId = session_id`, `AccountId` and `From = trc::Value::redacted()`):

```text
account_info = build_account_info(account)                 // error: trc::error!(ZA_IMIP_WRITE_FAILED), return
public_key = match wrap { Public(pk) => pk,
                          Session(_) | None => trace Error Reason=ZA_IMIP_SESSION_WRAP; return }
if itip_text.len() >= itip_inbound_max_ical_size: trace Error Details="iMIP message too large", Limit, Size; return
sender = za_imip_sender(..)                                 // None: return, as upstream
tree = itip_parse_inbound(itip_text, recipient)             // Err(e): trace Error Reason=itip_reason(&e), Details=ZA_IMIP_NOT_TAKEN; return
(uid, method) = from itip_sender_snapshot(&tree, &account_info.addresses, &sender)   // same Err handling
method not in {Request, Add, Cancel, Reply, Refresh}: trace Reason="UnsupportedMethod"; return
principal = za_account_id_from_email(&sender)               // Err: trc::error!(ZA_IMIP_LOOKUP_FAILED, account id only); return
if principal is a key account (za_is_key_account): trace Reason=ZA_IMIP_KEY_SENDER; return
if !za_imip_uid_known(account_id, uid):                     // Err: trc::error! as above; return
    if method != Request: trace Reason="EventNotFound"; return
    if !itip_auto_add_allowed(account_id, principal.is_some(), &sender): trace Reason="AutoAddDisabled"; return
match za_write_pending_item(CalendarAddress(""), account_id, &public_key, &tree, None, None, Some(&sender), None):
    Ok(true)  => trace Calendar(ItipMessageReceived)
    Ok(false) => trace Error Reason=ZA_IMIP_ITEM_REFUSED
    Err(e)    => trc::error!(e.account_id(account_id).details(ZA_IMIP_WRITE_FAILED))
```

- [ ] **Step 1: Write the failing delivery cases in `imip/inbound.rs`**

`pub async fn test(test: &mut TestServer)` creates `imip-key`, `imip-key2` (key) and `imip-plain` (plain, a local principal), gives `imip-key` a ContactCard for `ORG`, runs the cases in this order, then deletes its items, contacts and `imip-plain`. The drain stays skipped in this task.

```rust
/// Spec 4.1, 4.2, 8.2: an outside REQUEST gives one pending item beside the sealed mail.
async fn delivered_request(test: &TestServer, key: &Party) {
    let (before, mails) = (inbox_ids(test, key.id).await, mail_count(test, key.id).await);
    deliver("dmarc-org@remote.test", key.name, &itip_mail(ORG, key.name, "REQUEST",
        &event("imip-in-1", ORG, &[key.name], 0, ""))).await;
    wait_for_queue(test).await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1);
    let (id, item) = &items[0];
    assert_eq!(item.changed_by, ChangedBy::CalendarAddress(String::new()));
    assert_eq!(item.flags & (EVENT_NOTIFICATION_ZA_PENDING | EVENT_NOTIFICATION_PRIVATE),
        EVENT_NOTIFICATION_ZA_PENDING | EVENT_NOTIFICATION_PRIVATE);
    assert!(item.calendar_ids.is_empty() && item.event_id.is_none());
    assert_eq!(envelope_wrap(&item.event), Some(2), "sealed to the served key");
    assert!(!item.event.to_string().contains("imip-org"), "the sender only in the carrier");
    let (tree, carriers) = open_pending_item_as(test, key.id, STRONG, *id).await;
    assert_eq!(carriers.sender.as_deref(), Some(ORG));
    assert!(carriers.pair.is_none() && carriers.reply_token.is_none());
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    assert_eq!(mail_count(test, key.id).await, mails + 1, "the mail is stored whatever happens");
}

/// Spec 4.1 steps 1-5, 8.2: no item for any of these; each mail is still stored.
async fn refused_deliveries(test: &TestServer, key: &Party) {
    // (MAIL FROM, From, method, ics) per case, in this order:
    //  - no DMARC: NODMARC as MAIL FROM, ORG as From;
    //  - junk: `email_ingest` with `is_spam: true` (DmarcPass);
    //  - oversized: a REQUEST whose part is `itip_inbound_max_ical_size` bytes;
    //  - PUBLISH;
    //  - a key account's own mail: From `imip-key2@example.com`, ORGANIZER imip-key2;
    //  - an unknown sender's new UID with auto-add off: From STRANGER.
    for case in refused_cases(key) {
        let (before, mails) = (inbox_ids(test, key.id).await, mail_count(test, key.id).await);
        case.deliver(test).await;
        wait_for_queue(test).await;
        assert_eq!(inbox_ids(test, key.id).await, before, "{}: an item was written", case.label);
        assert_eq!(mail_count(test, key.id).await, mails + 1, "{}: the mail was not stored", case.label);
    }
}

/// Spec 4.1 step 5, D2: the stranger's new UID gets an item once it is a
/// ContactCard, and a local principal's (imip-plain over DMARC) gets one.
async fn auto_add_by_contact_and_principal(test: &TestServer, key: &Party)
// assertions: one new mail-borne item after each; the contact deleted afterwards.

/// Spec 4.1 step 5, D3: a CANCEL whose UID only a pending item names gets an item.
async fn known_uid_by_pending_item(test: &TestServer, key: &Party)
// REQUEST imip-in-d3 then CANCEL imip-in-d3, both delivered with no drain:
// assert_eq!(mail_borne_items(..) methods for the UID, [Request, Cancel]).

/// Spec 4.1 step 5: any other method for an unknown UID writes nothing.
async fn unknown_uid_other_methods(test: &TestServer, key: &Party)
// CANCEL and REPLY for UIDs no event or item names: inbox unchanged.

/// Review Focus 1.
async fn rf_1_provider_shaped_invitation(test: &TestServer, key: &Party)
// multipart/alternative with a base64 `text/calendar; method=REQUEST` part and an
// `application/ics` attachment of the same ICS: exactly one new item, and the
// opened tree's SUMMARY equals the decoded part's.
```

- [ ] **Step 2: Adjust the four existing modules (spec 8.5, spec question 1)**

- `fanout::test_task_queue`: "nothing in key2's inbox" becomes one mail-borne pending item from the plain organizer (`mail_borne_items` grows by one; its carrier's sender is `plain@example.com`), still no task for the key organizer, and the module's closing `delete_items_since` removes it.
- `fanout::test_plain_owner`: after each `wait_for_delivery`, key2 holds one more mail-borne pending item (the REQUEST, then the CANCEL); the module deletes both before its cleanup.
- `gating::test_scheduling`: after the plain organizer's invitation, key1 holds one pending mail-borne item at store level (`pending_ids` grows by one); the DAV listing of the inbox stays empty (pending items are hidden); the module deletes the item at its end. The copy after a drain is Task 5's.
- `mail/fence.rs` `forward_race_imip_side_write`: "the sealed retry" asserts no event and exactly one pending item with `envelope_wrap == Some(2)`, whose carrier's sender is the organizer, instead of no calendar data.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo check -p tests --tests`, then the za suite.
Expected: the tests compile (the helpers are test code); the za suite fails in `delivered_request` with `left: 0, right: 1`, since the gate still skips key accounts.

- [ ] **Step 4: Implement `za_imip.rs`'s three functions, the constants and the two `ingest.rs` edits**

The key branch reaches `za_imip_item` through `Box::pin`, as upstream reaches `itip_ingest`. `ItipMessages::queue` (`ingest.rs:933-936`) stays unreached for a key account: the branch pushes nothing to `itip_messages`.

- [ ] **Step 5: Run the tests and checks**

Run: `cargo test -p email --features test_mode za::`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite.
Expected: pass; no difference; `clean`; clean; the za suite passes.

- [ ] **Step 6: Commit**

```bash
git add crates/email/src/message/ingest.rs crates/email/src/message/za/mod.rs crates/email/src/message/za/imip.rs crates/common/src/za/imip.rs tests/src/za/imip tests/src/utils/za.rs tests/src/za/mod.rs tests/src/za/fanout.rs tests/src/za/gating.rs tests/src/za/mail/fence.rs
git commit -m "Release 5: a key account's inbound iMIP becomes a sealed mail-borne pending item"
```

### Task 5: The drain's mail arm

Implementer: `opus` (a kernel: upstream's merge applied with an untokened sender, the D7 end states, the REFRESH answer's ordering and the organizer's-copy gate).

**Files:**
- Modify: `crates/groupware/src/calendar/drain.rs` (`ItemCtx` `:508-519`; `Method` `:521-526`; `Incoming` `:528-535`; `drain_item` `:579-621`; `sender_candidates` `:786-789`; `import` `:892-1045`; `merge` `:1125-1572`; `ItemCtx::reject` `:1706-1720`; a new `apply_mail` after `apply`, `:751`)
- Modify: `crates/groupware/src/calendar/fanout.rs` (`za_imip_mail` after `za_mark_sent_by`, `:259`)
- Modify: `crates/common/src/za/imip.rs` (`ZA_IMIP_ITEM_DELETED`)
- Modify: `tests/src/za/imip/inbound.rs` (drain cases), `tests/src/za/imip/outbound.rs` (`refresh_two_sessions`, where 8.2 places it), `tests/src/za/gating.rs:651-674`

**Interfaces:**
- Consumes: Task 1 (`itip_fill_organizer`, `itip_sender_snapshot`, `itip_auto_add_allowed`); Task 2 (`ZaImipMail`, `za_imip_submit`); Task 3 (`PendingCarriers::sender`); Task 4's items.
- Produces, in `groupware::calendar::fanout`: `pub fn za_imip_mail(account_id: u32, document_id: u32, message: &ItipMessage<ICalendar>, recipient: &str) -> ZaImipMail`: `ItipMessages::new(vec![<the message with to = [recipient], its summary and tree cloned>])`'s one entry, so the contents are upstream's conversion (`scheduling/itip.rs:491-504`).
- Produces, in `groupware::calendar::drain` (private):
  - `Method` gains `Add` and `Refresh`; `drain_item` accepts them only when `carriers.sender.is_some()` (otherwise `delete_undecodable("UnsupportedMethod")` as today); `sender_candidates` maps `Add` to ORGANIZER and `Refresh` to ATTENDEE.
  - `Incoming` gains `Mail { sender: String }`.
  - `ItemCtx` gains `mail: bool`, set from the opened carriers before any write; with it, `reject` (`Reason = itip_reason(err)`, so the quota rejections of `commit_or_reject_quota` too) and a merge that changes nothing (`Reason = "NothingToMerge"`) delete the item through `delete_mail(reason)`: `Calendar(ItipMessageError)`, `AccountId`, `DocumentId`, `Reason = reason`, `Details = ZA_IMIP_ITEM_DELETED`, then `delete()`. Release 2 items (`mail == false`) keep every behaviour.
  - After `blocks_uid`, `drain_item` dispatches: a carrier sender goes to `async fn apply_mail(ctx: &ItemCtx<'_>, tree: ICalendar, sender: String, method: Method, uid: &str) -> trc::Result<bool>`, every other item to `apply` unchanged.
  - `import(ctx, tree, incoming: Incoming, uid, sender_addresses)` takes `Incoming::Request(pair)` from `apply` and `Incoming::Mail { sender }` from `apply_mail`; its re-dispatch to `merge` passes it through; the copy's pairs come only from `Request(Some(pair))`.
  - `merge` with `Incoming::Mail { sender }`: the sender address is the carrier's; no stale-reply check (D6 withdrawn); `MergeResult::Message` is answered (D14, below); pairs follow `Request(None)`'s rule; the delegating block (`:1309-1464`) runs for `Incoming::Reply(_)` as today and for `Incoming::Mail` only when the tree's method is REPLY and the stored event's ORGANIZER is one of the account's addresses (the stored snapshot's `organizer.email.is_local`, D15's gate).

`apply_mail`, which the signatures do not determine (4.3, D5, D7, D14):

```text
if method is Request or Reply: itip_fill_organizer(&mut tree, first address of the account)   // spec question 2
itip_sender_snapshot(&tree, addresses, &sender)                 // Err(e): ctx.reject → delete_mail(itip_reason(&e))
principal = za_account_id_from_email(&sender)                   // Err: return Err (counts toward the failure limit)
if principal is a key account: delete_mail(ZA_IMIP_KEY_SENDER)
existing = find_event(uid)
match (method, existing):
    (Request, None)        => if !itip_auto_add_allowed(.., principal.is_some(), &sender): delete_mail("AutoAddDisabled")
                              else import(ctx, tree, Incoming::Mail { sender }, uid, vec![sender])
    (_, Some(event_id))    => drain_lookup_pause_point (test mode, spec question 3); merge(ctx, event_id, tree, Incoming::Mail { sender }, vec![sender])
    (_, None)              => delete_mail("EventNotFound")
```

The REFRESH answer inside `merge` (D14): `ctx.delete()` first (its archive assertion admits one session); only after it returns `Ok(true)`, `za_imip_submit(server, za_imip_mail(account_id, event_id, &message, to))` for each `to` of upstream's message, traced by the seam; the deletion is traced with `Reason = "Refresh"`. No item, pin or status is written for it.

- [ ] **Step 1: Write the failing drain cases in `imip/inbound.rs`**

Each case holds `DrainOn` and triggers `imip-key`'s drain with `trigger_drain(KEY, STRONG)`; reads go through DAV (`copy_href`, GET).

```rust
/// Spec 4.3, D7: the Task 4 REQUEST imports; update, ADD and CANCEL merge;
/// the account's own DISPLAY alarm survives.
async fn drained_request_and_merges(test: &TestServer, key: &Party)
// after the drain: one copy of imip-in-1, the item drained (pending bit clear,
// changed_by still CalendarAddress("")); the client adds a DISPLAY VALARM; a REQUEST
// with SEQUENCE:1, a new SUMMARY and a weekly RRULE, an ADD (RFC 5546 3.2.4) of one
// new instance to that series, and a CANCEL, each drained in turn: the GET shows the
// new SUMMARY with the VALARM, then the added instance, then STATUS:CANCELLED; every
// item drained, none left pending.

/// Spec 4.3, 3.2 item 9, 4.6: REPLYs into the key organizer's event.
async fn replies(test: &TestServer, key: &Party)
// key PUTs imip-in-r (attendees ATT and DELEGATE); ATT's REPLY ACCEPTED carrying a
// second ATTENDEE line for DELEGATE with PARTSTAT=DECLINED: only ATT's PARTSTAT
// changes (ACCEPTED, SCHEDULE-STATUS 2.0), DELEGATE stays NEEDS-ACTION;
// key bumps SEQUENCE to 2; ATT's REPLY TENTATIVE at SEQUENCE:0 still applies;
// STRANGER's REPLY naming itself as ATTENDEE: one pending item first
// (assert_eq!(pending_ids(..).len(), 1)), then after the drain the item is gone
// and the event unchanged.

/// D3: REQUEST then CANCEL delivered while logged out (Task 4's items) end cancelled.
async fn logged_out_request_then_cancel(test: &TestServer, key: &Party)
// one drain: the copy of imip-in-d3 exists with STATUS:CANCELLED; both items drained.

/// D2: a ContactCard deleted before the drain deletes the item.
async fn contact_deleted_before_drain(test: &TestServer, key: &Party)
// contact for STRANGER added, REQUEST imip-in-d2 delivered (item written), contact
// deleted, drain: no copy of imip-in-d2, the item gone.

/// D14: a REFRESH is answered by mail, after the item's deletion.
async fn refresh_answer(test: &TestServer, key: &Party)
// sink().clear(); ATT's REFRESH for imip-in-r: after the drain and wait_for_queue the
// sink holds exactly one REQUEST to ATT for the UID, and the item is gone.

// In tests/src/za/imip/outbound.rs, holding its own DrainOn:
/// D14, 8.2: two sessions drain one REFRESH item; exactly one answer goes out.
async fn refresh_two_sessions(test: &TestServer, owner: &Party)
// owner PUTs imip-out-r with ATT; sink().clear(); ATT's REFRESH delivered; session 1
// (password, trigger_drain) parked
// at DRAIN_LOOKUP_PAUSE (HookParked with set_drain_lookup_pause); session 2 (an app
// password, za_app_password(owner.name, STRONG, "imip-second-session")) drains it;
// session 1 released; wait_for_queue:
// assert_eq!(sink().take_to(ATT).iter().filter(|m| is_request_for(m, uid)).count(), 1);

/// D15, 4.3 point 4: ATT's REPLY delegating to imip-key2 and DELEGATE.
async fn outside_delegation(test: &TestServer, key: &Party, key2: &Party)
// key PUTs imip-in-dl with ATT; sink().clear(); ATT's REPLY: PARTSTAT=DELEGATED;
// DELEGATED-TO="mailto:imip-key2@example.com","mailto:imip-delegate@remote.test",
// both delegates listed with DELEGATED-FROM ATT. After the organizer's drain:
// imip-key2 holds one pending fan-out item (PrincipalId), DELEGATE's line holds
// SCHEDULE-STATUS 5.3 and the sink holds nothing for DELEGATE. imip-key2 drains,
// accepts (PUT PARTSTAT=ACCEPTED); the organizer's next drain shows imip-key2
// ACCEPTED (its REPLY authenticated by the pair the delegating fan-out added).

/// 4.3 point 4, invariant 45, spec question 6: pairs are never touched by mail.
async fn pairs_untouched_by_mail(test: &TestServer, key: &Party, key2: &Party)
// key invites imip-key2 (internal) and ATT on imip-in-p; stored_pairs on key's copy
// before and after ATT's mail-borne REPLY merge: equal. imip-key2's copy (drained)
// holds its pair; an outside REQUEST for imip-in-p from ORG (ORGANIZER ORG) reaches
// imip-key2: its item is deleted after the drain (OrganizerMismatch) and the copy's
// pairs are equal before and after.

/// 4.3: an outside delegate's REPLY into a key delegator's copy merges and fans out nothing.
async fn delegate_reply_into_delegators_copy(test: &TestServer, key: &Party, key2: &Party)
// ORG invites key (REQUEST imip-in-sd, drained); key delegates to DELEGATE (PUT
// PARTSTAT=DELEGATED, DELEGATED-TO DELEGATE, ATTENDEE DELEGATE DELEGATED-FROM key);
// DELEGATE's REPLY (From DELEGATE) delegating on to imip-key2: after key's drain the
// copy shows DELEGATE's new PARTSTAT, and inbox_ids(imip-key2) is unchanged.

/// Spec 4.5: a plain local organizer's invitation. (The plain local attendee's REPLY
/// needs the outbound path: Task 7's `plain_attendee_round_trip`.)
async fn plain_local_organizer(test: &TestServer, key: &Party, plain: &Account)
// imip-plain PUTs imip-in-pl inviting key: wait_for_delivery (upstream's task); one
// mail-borne item (carrier sender imip-plain@example.com); key's drain imports it;
// imip-plain's update (new SUMMARY) reaches the copy after the next drain.

/// Review Focus 2.
async fn rf_2_sender_case(test: &TestServer, key: &Party)
// From `"Org" <IMIP-ORG@Remote.Test>`, ORGANIZER;CN=Org:mailto:imip-org@remote.test,
// MAIL FROM dmarc-org@remote.test: one item whose carrier is "imip-org@remote.test";
// the drain imports it.

/// Review Focus 4.
async fn rf_4_recurring_instance(test: &TestServer, key: &Party, plain: &Account)
// the same three emails (weekly series, one overridden instance, a CANCEL of another
// instance) to key and to imip-plain (who holds a ContactCard for ORG): after key's
// drain, the two GETs are equal up to SCHEDULE-STATUS, DTSTAMP and the account
// addresses.
```

- [ ] **Step 2: `gating::test_scheduling`'s copy after a drain**

Under `DrainOn`, after the item assertion of Task 4: `trigger_drain(key1)`; key1's calendar holds one copy of `za-invite-2`; the copy and the drained item are deleted before the RSVP block plants `copy.ics` (the same UID), and the RSVP refusal keeps its assertions.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo check -p tests --tests`, then the za suite.
Expected: `drained_request_and_merges` fails: the release 2 arm keeps the mail-borne REQUEST as history (`SenderIsNotOrganizerNorAttendee`), so no copy exists.

- [ ] **Step 4: Implement `za_imip_mail`, the mail arm, `Method`, `Incoming`, `ItemCtx::mail`, the D7 deletions and the D14 answer**

- [ ] **Step 5: Run the tests and checks**

Run: `cargo test -p groupware`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite.
Expected: pass; no difference; `clean`; clean; the za suite passes, the drain modules of release 2 unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/groupware/src/calendar/drain.rs crates/groupware/src/calendar/fanout.rs crates/common/src/za/imip.rs tests/src/za/imip/inbound.rs tests/src/za/imip/outbound.rs tests/src/za/gating.rs
git commit -m "Release 5: the drain's mail arm applies upstream's merge to mail-borne items"
```

### Task 6: Outbound routing: the mail class, `1.1` and the returned mail attempts

Implementer: `opus` (a kernel: which recipients are mailed, which are pinned, and the status each gets).

Why this is split from Task 7: routing (who is mailed, with which status) and ordering (when mail leaves relative to the commit) are two kernels a reviewer can judge apart, each with its own tests. After this task statuses read `1.1` and the fan-out returns its attempts, which the callers do not submit yet.

**Files:**
- Modify: `crates/groupware/src/calendar/fanout.rs` (`ZaFanOutResult` `:132-143`; the trait and impl signatures `:261-316`; phase 1 `:370-470`; statuses `:620-729`; `Recipient` `:1147-1155`; `Route` `:1157-1176`; `route` `:1180-1215`; `status_rank` `:1320-1327`; `resolve_inner` `:1377-1435`; `mod tests`)
- Modify: `crates/dav/src/calendar/update.rs:717-733`, `:1404-1420` (`internal_only: false`); `crates/dav/src/calendar/delete.rs:480-496` (`false`); `crates/groupware/src/calendar/drain.rs:1397-1418` (`true`); `crates/http/src/api/contacts.rs:753-773` (`true`)
- Modify: `tests/src/za/imip/outbound.rs` (`statuses`), `tests/src/za/fanout.rs:270-275` (and the module doc's class description), `tests/src/za/sharing.rs:7721-7729`, `tests/src/za/tracing.rs:887-895`

**Interfaces:**
- Consumes: Task 2 (`ZaImipMail`); Task 5 (`za_imip_mail`).
- Produces, in `groupware::calendar::fanout`:
  - `pub const ITIP_STATUS_SENT: &str = "1.1";`; `status_rank` gains an explicit `ITIP_STATUS_SENT | ITIP_STATUS_DELIVERED => 0` arm.
  - `Recipient::Mail`, returned by `resolve_inner` where it returns `NotInternal` today for an address that resolves to no local account (`za_account_id_from_email` is `None`, `ZaResolvedAccount::NotFound`, an id `try_account` no longer finds) and for a local account that is not a key account; the self, excluded-owner, permission and served-key returns stay `NotInternal`, the lookup failures `Failed`.
  - `Route::Mail { on_organizer: bool }`; `fn route(method, message, recipient, organizer, union, mail: bool) -> Route`: with `mail`, `Route::Mail { on_organizer: <recipient is the ORGANIZER> }` for every method; without, release 2's routes unchanged.
  - Phase 1 resolves every normalizable address of every message once, then routes it with `mail = matches!(recipient, Recipient::Mail)`; `internal` gains a recipient only from an attempt routed `Deliver`, so nothing new is decided or pinned (5.2).
  - `za_fan_out(&self, sender, writer_account, event_account, keys, messages, stored_pairs, event, source, kind: ZaFanOutKind, internal_only: bool, policy, fence, resolution)`: a `Route::Mail` attempt records `ITIP_STATUS_NOT_ATTEMPTED` under `internal_only` and yields nothing; otherwise it records `ITIP_STATUS_SENT` and pushes `za_imip_mail(event_account.account_id, source.document_id, message, address)`, built from the reconciled, `SENT-BY`-marked message.
  - `ZaFanOutResult` gains `pub mail: Vec<ZaImipMail>`, one per (message, mail-class address), in message order.

- [ ] **Step 1: Write the failing unit tests in `fanout.rs`'s `mod tests`**

The existing `route_of` helper (`:1634`) gains the `mail` argument.

```rust
#[test]
fn a_mail_recipient_gets_every_message_and_internal_ones_keep_release_2_routes() {
    let organizer = Email::new("mailto:org@example.com", &organizer_addresses());
    let union = TokenPairs::new(); // no token for anyone
    // (message, recipient, routed Deliver when internal)
    let cases = [
        (message("att@remote.test", false, "REPLY"), "org@example.com", false),   // token-less REPLY to the ORGANIZER
        (message("att@remote.test", false, "REQUEST"), "dlg@remote.test", false), // attendee-originated REQUEST
        (message("dlg@remote.test", false, "REPLY"), "att@remote.test", false),   // REPLY to a delegator
        (message("org@example.com", true, "CANCEL"), "att@remote.test", true),
    ];
    for (msg, to, deliver) in &cases {
        assert!(matches!(route_of(msg, to, organizer.as_ref(), &union, true), Route::Mail { .. }), "{to}");
        assert_eq!(matches!(route_of(msg, to, organizer.as_ref(), &union, false), Route::Deliver { .. }),
            *deliver, "{to}: release 2's route");
    }
    assert!(matches!(route_of(&cases[0].0, "org@example.com", organizer.as_ref(), &union, true),
        Route::Mail { on_organizer: true }));
}

#[test]
fn sent_ranks_below_every_failure() {
    assert!(status_rank(ITIP_STATUS_SENT) < status_rank(ITIP_STATUS_NOT_ATTEMPTED));
    assert!(status_rank(ITIP_STATUS_SENT) < status_rank(ITIP_STATUS_DELIVERY_FAILED));
    assert_eq!(status_rank(ITIP_STATUS_SENT), 0);
}
```

- [ ] **Step 2: Write the failing `statuses` case in `imip/outbound.rs` and adjust the existing assertions**

`imip::outbound::test` now also creates `imip-out-key2`, `imip-out-plain`, `imip-noperm`, `imip-pending` and the list `imip-list` (members `imip-out-key2` and `imip-out-plain`).

```rust
/// Spec 5.1, 5.4, D9, decision 10: the status of each class.
async fn statuses(test: &TestServer, owner: &Party) {
    let path = format!("/dav/cal/{}/default/imip-out-1.ics", owner.name.replace('@', "%40"));
    assert_eq!(uid_lease::put(owner.name, STRONG, path.clone(), event("imip-out-1", owner.name, &[ATT,
        "imip-out-plain@example.com", "imip-out-key2@example.com", "imip-pending@example.com",
        "imip-noperm@example.com", "imip-out-key+sub@example.com", "imip-list@example.com"], 0, "")).await, 201);
    for (address, status) in [(ATT, "1.1"), ("imip-out-plain@example.com", "1.1"),
        ("imip-list@example.com", "1.1"), ("imip-out-key2@example.com", "1.2"),
        ("imip-pending@example.com", "5.3"), ("imip-noperm@example.com", "5.3"),
        ("imip-out-key+sub@example.com", "5.3")] {
        assert_eq!(attendee_statuses(&owner.client, &path, address).await, [status], "{address}");
    }
}
```

`fanout::test` (`:270-275`): plain and the outsider read `["1.1"]`; `:340` (the sender's sub-address) and `:994` (no permission) stay `["5.3"]`; its "plain received mail" count is untouched in this task. `sharing.rs:7721-7729`: the `sole-group` label's `sharing-life-team@example.com` statuses are all `1.1` (Jay's decision 10). `tracing.rs:887-895`: `LOOKUP_LOCAL_ATTENDEE` and `SUBADDR_ATTENDEE` read `["1.1"]`.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p groupware fanout`, then `cargo check -p tests --tests` and the za suite.
Expected: compile errors naming `ITIP_STATUS_SENT` and `route`'s sixth argument; then `statuses` fails on `imip-att@remote.test` with `["5.3"]`.

- [ ] **Step 4: Implement the class, the route, the flag and the returned attempts; pass the flag at every caller**

- [ ] **Step 5: Run the tests and checks**

Run: `cargo test -p groupware`; `cargo test -p http@1.0.1 --lib --bins --features test_mode,enterprise`; the formatting check (http's form included); the per-task compile check; `cargo check -p tests --tests`; the za suite.
Expected: pass; pass; no difference; `clean`; clean; the za suite passes with the adjusted assertions.

- [ ] **Step 6: Commit**

```bash
git add crates/groupware/src/calendar/fanout.rs crates/groupware/src/calendar/drain.rs crates/dav/src/calendar/update.rs crates/dav/src/calendar/delete.rs crates/http/src/api/contacts.rs tests/src/za/imip/outbound.rs tests/src/za/fanout.rs tests/src/za/sharing.rs tests/src/za/tracing.rs
git commit -m "Release 5: the mail recipient class, status 1.1 and the fan-out's mail attempts"
```

### Task 7: Outbound submission after the commit

Implementer: `opus` (a kernel: mail leaves only after the sender's commit returned `Ok`, on every path).

**Files:**
- Modify: `crates/dav/src/calendar/update.rs` (update: capture `fan_out.mail` at `:714-739`, the hook after the pause point at `:779-780`, submit in the `Ok` arm at `:782-787`; create: capture at `:1402-1447`, submit after `notify_task_queue` at `:1566`)
- Modify: `crates/dav/src/calendar/delete.rs` (collect each fan-out's `mail` at `:480-498`; the hook in place of the commit's result at `:506`; on `Ok` and on the retry's `Ok`, submit after `:555`; on any error, trace the drop before returning)
- Modify: `crates/common/src/za/test_hooks.rs` (`UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_FAIL_ONCE`; re-export `set_second_chunk_fail_once`)
- Modify: `crates/common/src/za/imip.rs` (`za_imip_trace_dropped`, `ZA_IMIP_DROPPED`)
- Modify: `crates/store/src/za_test.rs` (`SECOND_CHUNK_FAIL_ONCE`, its setter and `take_`, a unit test)
- Modify: `crates/store/src/dispatch/store.rs:299-311` (one test-mode check before the second chunk)
- Modify: `tests/src/za/imip/outbound.rs`, `tests/src/za/fanout.rs`, `tests/src/za/gating.rs:505-650`, `tests/src/za/tracing.rs:871-1135`, `tests/src/za/leak.rs` (`SCHED_EVENT` `:214-221`; `test_drain`, `test`: sink and purge), `tests/src/za/pinning.rs:1205-1220`, `tests/src/za/sharing.rs:5668-5680`, `tests/src/utils/za.rs` (`expire_and_purge_blobs` lifted from `mail/fence.rs:527`, which then imports it; `clear_received_scheduling`)

**Interfaces:**
- Consumes: Task 2 (`za_imip_submit_all`); Task 6 (`ZaFanOutResult::mail`).
- Produces, in `common::za::test_hooks`: `UPDATE_COMMIT_FAIL_ONCE`, `set_update_commit_fail_once(Option<u32>)`, `take_update_commit_fail(u32) -> bool`; `DELETE_COMMIT_FAIL_ONCE`, `set_delete_commit_fail_once(Option<u32>)`, `take_delete_commit_fail(u32) -> bool`; `pub use store::za_test::set_second_chunk_fail_once`.
- Produces, in `store::za_test`: `pub static SECOND_CHUNK_FAIL_ONCE: Mutex<Option<u32>>`, `pub fn set_second_chunk_fail_once(account_id: Option<u32>)`, `pub fn take_second_chunk_fail(account_id: Option<u32>) -> bool` (only an exact match of the batch's current account; one-shot).
- Produces, in `common::za::imip`: `pub fn za_imip_trace_dropped(account_id: u32, mails: &[ZaImipMail])`: one `Calendar(ItipMessageError)` per distinct document id, `AccountId`, `DocumentId`, `Details = ZA_IMIP_DROPPED`.
- Produces, in `tests/src/utils/za.rs`: `pub async fn expire_and_purge_blobs(test: &TestServer, admin: &Account)` (moved unchanged); `pub async fn clear_received_scheduling(test: &TestServer, account: &Account)`: `wait_for_queue`, then the account's messages (`destroy_all_mailboxes_for_account`), its scheduling inbox items and the calendars `itip_ingest` created (its JMAP client's `destroy_all_event_notifications` and `destroy_all_calendars`), for a plain account that outlives the module.
- The callers (5.4, 5.5, decision 9):
  - Update PUT (key path): `UPDATE_COMMIT_FAIL_ONCE` taken on attempt 0 yields `Err(StoreEvent::UnexpectedError.details("zero-access test hook: update commit failure"))` in place of `commit_batch`'s result, so it takes the existing error arm; `za_imip_submit_all` runs in the `Ok` arm after `notify_task_queue`, so after the bounded retry's commit too.
  - Create PUT: after `notify_task_queue` (`:1566`), reached only when the commit (or `za_commit_create`'s retry) returned `Ok`; `COMMIT_FAIL_ONCE` and `za_lost_create_fence` return before it.
  - DELETE (single event and collection): `DELETE_COMMIT_FAIL_ONCE`, taken when `za_keys.is_some()`, yields `Err(StoreEvent::UnexpectedError.details("zero-access test hook: delete commit failure"))` in place of `commit_batch`'s result; every error return of the commit `match` first calls `za_imip_trace_dropped`; `za_imip_submit_all` runs after the `match` returned (the first commit or the retry's).
  - `Store::write_batch`: `let account_id = builder.last_account_id();` before `commit_points()`, and in the loop `#[cfg(feature = "test_mode")] if chunk == 1 && crate::za_test::take_second_chunk_fail(account_id) { return Err(StoreEvent::UnexpectedError.into_err().details("zero-access test hook: second chunk failure")) }`.

- [ ] **Step 1: Write the failing store unit test in `za_test.rs`**

```rust
/// Spec 8.2: a forced-split batch whose second chunk fails keeps its first
/// chunk, returns the error, and the hook is spent; another account's batch is untouched.
#[tokio::test]
async fn the_second_chunk_fails_once_for_its_account() {
    const FAILS: u32 = 0x5a5a_0301;
    const OTHER: u32 = 0x5a5a_0302;
    let store: Store = EphemeralStore::open().into();
    set_force_commit_split(Some(FAILS));
    set_second_chunk_fail_once(Some(FAILS));
    assert!(store.write_batch(&mut batch_of(FAILS)).await.is_err());
    set_force_commit_split(None);
    assert_eq!(written_documents(&store, FAILS).await, [0], "the first chunk stands");
    assert!(!take_second_chunk_fail(Some(FAILS)), "one-shot");
    set_second_chunk_fail_once(Some(FAILS));
    assert!(store.write_batch(&mut batch_of(OTHER)).await.is_ok());
    set_second_chunk_fail_once(None);
}
```

- [ ] **Step 2: Write the failing `imip::outbound` cases**

`imip::outbound::test` now also creates `imip-grantee` and `imip-alias`, gives `imip-out-key2` and `imip-alias` a ContactCard for `ORG`, and holds `DrainOn`. Each case calls `sink().clear()` first and deletes its events (`NO_REPLY` where its CANCEL must not go out).

```rust
/// Spec 5.4, 5.6, invariant 41: Task 6's PUT now mails ATT, imip-out-plain and
/// imip-list once each, with no task (the consumer held), no RSVP link; the plain
/// attendee and the plain member import it; the key member's mail is sealed and its
/// inbox gets no item (decision 10).
async fn mailed_once(test: &TestServer, owner: &Party)
// held = hold_task_consumer; the statuses PUT; assert_eq!(itip_tasks_for(owner.id), 0);
// drop(held); wait_for_queue; sink().count_to(ATT) == 1; no html part contains
// "/calendar/rsvp"; mail_count(imip-out-plain) grew by 2 (direct and list);
// uid_copies(imip-out-plain, "imip-out-1") == 1; imip-out-key2's mailbox holds one
// stub (row_is_stub) and its inbox gained only the internal item (PrincipalId).
// Control: imip-out-plain organizing imip-out-c inviting ATT: after
// wait_for_delivery, the sink copy's html contains "/calendar/rsvp?i=".

/// Spec 4.5, 5.6: the plain local attendee's REPLY returns through upstream's task
/// and mail into the key organizer's event.
async fn plain_attendee_round_trip(test: &TestServer, owner: &Party, plain: &Account)
// imip-out-plain's imported copy of imip-out-1 (from mailed_once), PUT with
// PARTSTAT=ACCEPTED: after wait_for_delivery, owner's inbox holds one mail-borne item
// (carrier sender imip-out-plain@example.com); after owner's drain the GET of
// imip-out-1 shows imip-out-plain ACCEPTED with SCHEDULE-STATUS 2.0.

/// Spec 5.5: an update and a DELETE mail REQUEST and CANCEL.
async fn update_and_delete(test: &TestServer, owner: &Party)
// the event GET, SUMMARY changed, PUT: the sink holds one REQUEST to ATT with the new
// SUMMARY; DELETE: one CANCEL to ATT.

/// D10: a key attendee's ACCEPTED, DECLINED and DELETE mail REPLYs to ORG.
async fn attendee_replies(test: &TestServer, attendee: &Party)
// ORG's REQUEST imip-out-a delivered and drained into imip-out-key2; PUT ACCEPTED,
// then DECLINED, then DELETE: three REPLYs to ORG at the sink, PARTSTAT ACCEPTED,
// DECLINED, DECLINED, each From imip-out-key2.

/// D10: a key delegator's REQUEST to an outside delegate.
async fn delegation_to_outside(test: &TestServer, attendee: &Party)
// on a drained copy of ORG's imip-out-d, PUT PARTSTAT=DELEGATED with DELEGATED-TO
// DELEGATE: the sink holds a REPLY to ORG (DELEGATED) and a REQUEST to DELEGATE.

/// D12, release 4: a grantee's write mails from the owner's address with SENT-BY.
async fn grantee_write(test: &TestServer, owner: &Party, grantee: &Party)
// pin then shares/grant (level "edit") of owner's calendar imip-shared, as
// sharing.rs:2769-2789 does; the grantee PUTs imip-out-g with ORGANIZER the owner and
// ATT: the sink message's MAIL FROM and From are the owner's address, its ICS
// ORGANIZER carries SENT-BY="mailto:imip-grantee@example.com".

/// Decision 9: a failed submission leaves 1.1 and is traced.
async fn submission_failure(test: &TestServer, owner: &Party)
// set_imip_submit_fail_once(Some(owner.id)); PUT imip-out-f inviting ATT: 201, ATT's status
// ["1.1"], nothing at the sink, one Calendar(ItipMessageError) with Details
// ZA_IMIP_FAILED (a trace subscriber as tracing.rs uses).

/// Decision 9, 8.2: a commit failure after the fan-out mails nothing, on each path.
async fn commit_failures(test: &TestServer, owner: &Party)
// COMMIT_FAIL_ONCE on a create, UPDATE_COMMIT_FAIL_ONCE on an update,
// DELETE_COMMIT_FAIL_ONCE on a single-event DELETE (each inviting ATT with a distinct
// SUMMARY canary): each request fails, and after wait_for_queue no sink message
// carries that canary.

/// 8.2: the store's own commit fails after every precommit step.
async fn store_commit_failures(test: &TestServer, owner: &Party)
// update: PUT "loser-update" parked by HookParked(set_update_commit_pause) via
// uid_lease::put; a competing PUT of the same event commits ("winner-update");
// release: the loser fails, and no sink message carries "loser-update".
// single-event DELETE: parked by set_delete_commit_pause (a raw DELETE); a competing
// PUT commits; release: the DELETE fails, and the sink holds no CANCEL for the UID.
// create: set_lease_secs_override(Some(1)); a create PUT "loser-create" parked by
// set_commit_pause; after 1.5 s another create of the same UID under another href
// takes the lease and commits; release: the loser fails through
// za_lost_create_fence, and no sink message carries "loser-create"; override cleared.

/// 5.4: a chunked collection DELETE whose second chunk fails mails nothing.
async fn chunked_collection_delete(test: &TestServer, owner: &Party)
// calendar imip-coll with two events inviting ATT; ForceCommitSplit::new(owner.id);
// set_second_chunk_fail_once(Some(owner.id)); the collection DELETE (raw client)
// fails; the split guard dropped; the first chunk's event is gone, the second remains,
// no CANCEL at the sink, and the trace holds ZA_IMIP_DROPPED for the batch's events.
// Cleanup: the remaining event deleted with NO_REPLY, then the calendar.

/// Review Focus 3.
async fn rf_3_alias_attendee(test: &TestServer, alias: &Party)
// ORG's REQUEST to imip-alias-two@example.com, drained, accepted: one REPLY to ORG
// at the sink, MAIL FROM and From imip-alias-two@example.com.

/// Review Focus 5.
async fn rf_5_many_recipients(test: &TestServer, owner: &Party)
// imip-many-01..20@remote.test via uid_lease::put: 201 within its 5 s; every status
// ["1.1"]; after wait_for_queue the sink holds 20 messages, one recipient each.
```

- [ ] **Step 3: Adjust the existing modules (spec 8.5)**

- `fanout::test`: plain and the outsider are mailed once by the mixed PUT (the outsider at the sink; plain's mail count grows by one) instead of "plain received mail" staying flat (`:279-284`); the later updates of `fo-mixed` and its DELETE mail them again, so `:515`'s count grows by the CANCEL; `:340` and `:994` keep `5.3` and no mail; before returning, `clear_received_scheduling(test, &plain)`.
- `gating::test_scheduling` (8.5): the key organizer's invitation reaches plain (plain's mail count grows by one, its inbox lists one notification and its calendar one copy of `za-invite-1`) instead of nothing leaving; the planted invitation's DELETE mails plain its CANCEL; the RSVP refusal stays; before returning, `clear_received_scheduling(test, &plain)`.
- `leak.rs`'s `SCHED_EVENT` (spec question 7): the plain ATTENDEE line becomes `ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;SCHEDULE-AGENT=CLIENT:mailto:plain@example.com`, and its doc comment says why.
- `tracing::test`: the PUT's two unknown local attendees are refused at RCPT after the commit, verified by two `Calendar(ItipMessageError)` events with `Reason = "RCPT-TO"` and a 5xx `Code`; the external attendee reaches the sink. The never-appears checks of `LOOKUP_LOCAL_CANARY`, `LOOKUP_DOMAIN_CANARY`, `SUBADDR_CANARY` and `SUBADDR_DOMAIN_CANARY` (`:1116-1135`) run over the events outside the submissions' spans: an event is inside when its `SpanId` is the span of an `Smtp`, `Delivery` or `Queue` event that names the canary. The module's other never-appears controls stay as they are; any event naming the canaries outside those spans, including one emitted before the PUT's commit returns, fails.
- `leak::test_drain` and `leak::test` (canary events with `remote.test` attendees): `wait_for_queue`, then `expire_and_purge_blobs`, before each scan. `leak::test_drain` is the first leak module, so its purge also removes the queue blobs that earlier modules' outbound mail left (`fanout`, `pinning`, `sharing`, `tracing`).
- `pinning::case_11_mixed` (`dave@external.test`) and `sharing.rs:5673` (`task9-unknown@not-local.test`): `wait_for_queue` after the write, then `assert!(!sink().take_to(<address>).is_empty())`.
- Any other existing assertion the suite run shows failing only because a recipient is now mailed is adjusted under the Global Constraints rule and listed in the report.

- [ ] **Step 4: Run them to see them fail**

Run: `cargo test -p store --lib --features test_mode za_test`; `cargo check -p tests --tests`; the za suite.
Expected: compile errors naming `set_second_chunk_fail_once` and the two commit hooks; then `mailed_once` fails: nothing at the sink for `imip-att@remote.test`.

- [ ] **Step 5: Implement the hooks, `za_imip_trace_dropped`, the `write_batch` line and the submissions at the three callers**

- [ ] **Step 6: Run the tests and checks**

Run: `cargo test -p store --lib --features test_mode za_test`; `cargo test -p common za::`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite.
Expected: pass; pass; no difference; `clean`; clean; the za suite passes.

- [ ] **Step 7: Commit**

```bash
git add crates/dav/src/calendar/update.rs crates/dav/src/calendar/delete.rs crates/common/src/za/test_hooks.rs crates/common/src/za/imip.rs crates/store/src/za_test.rs crates/store/src/dispatch/store.rs tests/src/za/imip/outbound.rs tests/src/za/fanout.rs tests/src/za/gating.rs tests/src/za/tracing.rs tests/src/za/leak.rs tests/src/za/pinning.rs tests/src/za/sharing.rs tests/src/za/mail/fence.rs tests/src/utils/za.rs
git commit -m "Release 5: outbound iMIP is submitted after the sender's commit, with commit-failure hooks"
```

### Task 8: The other harnesses

Implementer: `sonnet` (verification and harness work; the plan carries the audit and the expected results).

**Files:**
- Modify only if a run shows queued mail: the harness's setup (for `webdav_tests`, `tests/src/webdav/mod.rs` calls `crate::za::imip::route_outside_mail(&admin, &test.server)` before its reload, `:204`).

**Interfaces:**
- Consumes: Tasks 1-7.
- Produces: one record per harness for the outcome note: routed to a sink, or why no outside attendee reaches a key account's scheduling there.

- [ ] **Step 1: Audit each harness for attendees a key account would now mail**

Run: `grep -n "ORGANIZER\|ATTENDEE" tests/src/webdav/cal_query.rs tests/src/webdav/za_variants.rs`; `grep -rn "text/calendar\|METHOD:REQUEST" tests/src/imap tests/src/jmap`; `grep -n "attendee\|organizer" tests/caldav_client/*.py`; `grep -n "ATTENDEE\|ORGANIZER" tests/product/fresh_store.py`.
Expected, as read at `1b1cd094d`: `cal_query`'s events are dated 2006 and name neither its owner as organizer nor as attendee (no message is built); `za_variants::scheduling` invites key accounts only (internal); the mail suites carry no iMIP part and key-mode JMAP refuses calendars; the CalDAV client invites only the harness's own users (key accounts in key mode); the fresh store's events have no attendee. Record each line; a hit not explained this way is routed to a sink in Step 3.

- [ ] **Step 2: Run the key-mode suites**

Run, one at a time, with `ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216`: `webdav::webdav_tests`, `imap::imap_tests`, `jmap::jmap_tests`.
Expected: each passes; key-mode `webdav_tests`' closing `assert_is_empty` finds no `QueueMessage` key.

- [ ] **Step 3: Route a harness to the sink only if Step 2 or 4 shows queued mail**

For `webdav_tests`, the one call above; for a Python harness, record the finding and stop for Jay (the harness is not this plan's to change).

- [ ] **Step 4: Run the CalDAV client and the fresh-store gate against the product build**

Run: the product build; the fresh store's failure tests `python -B -m unittest discover -s tests/product -p test_fresh_store.py -v` and `python -B tests/product/fresh_store.py --binary target/release/stalwart --log-dir <a directory under the session's scratchpad>`; the CalDAV client as CI runs it (`.github/workflows/test.yml:436-458`: python-caldav at `111ae14b875f40d100d5259820f10210f00e20f9` in a venv under the session's scratchpad, then `tests/caldav_client/runner.py --binary target/release/stalwart --caldav-source <checkout> --log-dir <dir>`). If the venv's packages are not available locally, ask Jay before installing anything and record the step as pending for CI's `caldav-client` job.
Expected: the fresh store reports "work drained: SMTP queue empty" and its readback passes; the CalDAV client's key mode is 1 skipped, 2 xfailed, and plain mode carries its five records (`docs/zero-access/client-caldav.md`).

- [ ] **Step 5: Commit, only if Step 3 changed a file**

```bash
git add tests/src/webdav/mod.rs
git commit -m "Release 5: route key-mode CalDAV suite mail to the sink"
```

Otherwise record "no change" in the ledger with the four records.

### Task 9: The differential oracle

Implementer: `sonnet` (test-harness work against a fully specified exchange; the reviewer runs at `opus`, since this is the release's stopping criterion, spec 3.4).

**Files:**
- Create: `tests/src/za/imip/oracle.rs`
- Modify: `tests/src/za/imip/mod.rs` (`pub mod oracle;`), `tests/src/za/mod.rs` (`imip::oracle::test` after `imip::outbound::test`)

**Interfaces:**
- Consumes: Tasks 2-7; `smtp_sink`; `trigger_drain`; `DrainOn`.
- Produces: `pub async fn test(test: &mut TestServer)`.

- [ ] **Step 1: Write the oracle**

Setup: `imip-oracle-key` (key, password `STRONG`) and `imip-oracle-plain` (plain), each with a ContactCard for `ORG` (created first, deleted at the end, 8.3's precondition); `httpRsvpEnable` turned off (`registry_update_setting(CalendarScheduling { http_rsvp_enable: false, .. }, &[Property::HttpRsvpEnable])` and a reload) and restored to `true` at the end; `DrainOn` for the run.

The exchange (3.3), identical for both accounts (same UID `imip-oracle-in`, then `imip-oracle-out`, same content), one step at a time:

| Step | Inbound (ORG organizes) | Outbound (the account organizes, ATT attends) |
|---|---|---|
| 1 | ORG's REQUEST delivered to each | PUT inviting ATT |
| 2 | ORG's update (SEQUENCE 1, new SUMMARY and time) | ATT's emailed REPLY ACCEPTED delivered to each |
| 3 | the account accepts in its client (PUT PARTSTAT=ACCEPTED) | update by PUT (new SUMMARY) |
| 4 | ORG's CANCEL | DELETE |

After each step: `trigger_drain(imip-oracle-key)`, `wait_for_delivery`; then

```rust
assert_eq!(present(plain), present(key), "step {n}: the object on both sides"); // absent on both after a DELETE
assert!(key_items(test).await.iter().all(|(_, i)| i.flags & EVENT_NOTIFICATION_ZA_PENDING == 0), "step {n}: drained");
assert_eq!(new_items(plain).len(), new_items(key).len(), "step {n}: inbox items");
assert_eq!(new_sink(plain).len(), new_sink(key).len(), "step {n}: sink messages");
assert_eq!(normalize(&dav_get(plain_event)), normalize(&dav_get(key_event)), "step {n}: stored event");
for (p, k) in new_items(plain).iter().zip(new_items(key)) {      // each read with a DAV GET of the inbox item
    assert_eq!(normalize(p), normalize(&k), "step {n}: item tree");
}
for (p, k) in new_sink(plain).iter().zip(new_sink(key)) {        // matched by recipient and method
    for part in [From, To, Subject, AutoSubmitted, ReplyTo, Method, Text, Html, Ics] {
        assert_eq!(normalize(&p.get(part)), normalize(&k.get(part)), "step {n}: {part:?}");
    }
}
```

`normalize` applies exactly 3.3's permitted differences and nothing more: addresses, display names and hrefs mapped `imip-oracle-plain` → `imip-oracle-key` (both directions where a value names the account); Message-ID, Date, MIME boundaries, the logo CID, DTSTAMP and imported file names replaced by placeholders; `SCHEDULE-STATUS` parameters removed. Any other difference fails.

- [ ] **Step 2: Run it**

Run: the za suite.
Expected: `imip::oracle` passes. A failure is either a fork bug (fix it in the task that owns the code, back through its reviewer) or a permitted difference the normalizer lacks (only if 3.3 lists it); record either in the task report.

- [ ] **Step 3: Check that the oracle is not blind**

A self-check inside the module, before the exchange: `normalize` of two copies of one stored event that differ only in DTSTAMP, an address of the mapping and a `SCHEDULE-STATUS` compares equal, and of two that also differ in SUMMARY compares unequal. Then a kill check against the branch head commit: in `merge`, `Incoming::Mail` maps `MergeResult::Actions` to `Step::Nothing`; run the za suite; record the first failing assertion (expected: inbound step 2's stored event, the update not applied on the key side); revert, `git status --short crates/` empty.

- [ ] **Step 4: Commit**

```bash
git add tests/src/za/imip/oracle.rs tests/src/za/imip/mod.rs tests/src/za/mod.rs
git commit -m "Release 5: the differential oracle against a plain account"
```

### Task 10: The trace module

Implementer: `sonnet` (a test module over section 7's table).

**Files:**
- Create: `tests/src/za/imip/traces.rs`
- Modify: `tests/src/za/imip/mod.rs` (`pub mod traces;`), `tests/src/za/mod.rs` (`imip::traces::test` after `imip::oracle::test`)

**Interfaces:**
- Consumes: Tasks 2-7; the subscriber pattern of `tests/src/za/tracing.rs` (`Collector` subscriber, `strings`, the 500 ms settle).
- Produces: `pub async fn test(test: &mut TestServer)` (account `imip-trace-key`).

- [ ] **Step 1: Write the module**

With canaries `imiptrace-sum-canary` (SUMMARY), `imiptrace-loc-canary` (LOCATION), the sender `imiptrace-from-canary@remote.test` (ORGANIZER and From, its display name `imiptrace-name-canary`) and the attendee `imiptrace-att-canary@remote.test`, subscribe to every event, then, under `DrainOn`: deliver a REQUEST (item written), a REQUEST from a key account (refused), an oversized part (refused), a CANCEL for an unknown UID (refused), drain an item that ends deleted (a REPLY from a stranger), PUT an invitation to the attendee canary (submitted), PUT with `IMIP_SUBMIT_FAIL_ONCE` (failed), DELETE with `DELETE_COMMIT_FAIL_ONCE` (dropped). Then:

```rust
let ours = events_of(&typed, [ItipMessageReceived, ItipMessageError, ItipMessageSent], trace_key.id);
assert!(ours.len() >= 8, "the scenario's events were captured");
for event in &ours {
    assert!(!event.text.contains('@') && !TRACE_CANARIES.iter().any(|c| event.text.contains(c)), "{event:?}");
    if let Some(from) = event.value(Key::From) { assert_eq!(from, "[redacted]"); }
}
assert!(ours.iter().any(|e| e.has(Key::QueueId) && e.has(Key::DocumentId)), "sent: account, document, queue id");
assert!(ours.iter().any(|e| e.reason() == Some("zero-access: iMIP from a key account")));
assert!(ours.iter().any(|e| e.has(Key::Limit) && e.has(Key::Size)), "the size refusal");
assert!(ours.iter().any(|e| e.details() == Some(ZA_IMIP_DROPPED)));
assert!(!all.contains("imiptrace-sum-canary") && !all.contains("imiptrace-loc-canary"), "no content anywhere");
assert!(outside_smtp_spans(&typed, "imiptrace-att-canary").is_empty(), "only the SMTP layer's envelope names the recipient");
assert!(!all.contains("imiptrace-from-canary") || only_in_smtp_spans(&typed, "imiptrace-from-canary"));
```

`outside_smtp_spans` returns the events naming the canary whose `SpanId` is not the span of an `Smtp`, `Delivery` or `Queue` event naming it (Task 7's rule).

- [ ] **Step 2: Run it, then kill-check it**

Run: the za suite. Expected: pass. Kill check against the branch head commit: `za_imip_item`'s `From = trc::Value::redacted()` becomes `From = sender.as_str()`; expected catcher: the `'@'` assertion; revert, `git status --short crates/` empty.

- [ ] **Step 3: Commit**

```bash
git add tests/src/za/imip/traces.rs tests/src/za/imip/mod.rs tests/src/za/mod.rs
git commit -m "Release 5: the iMIP trace module"
```

### Task 11: The leak module

Implementer: `sonnet` (an extension of the existing scanner to the new carrier and the sink's copies).

**Files:**
- Modify: `tests/src/za/leak.rs` (`EXTRA_CANARIES` `:150-204`; `carrier_kind` `:706-718`, `check_item` `:834-887`, `ends_extra_then_key` `:817-832` accept `X-ZA-SENDER` as a pending item's one carrier; the stored-trace exclusion; `pub async fn test_imip`)
- Modify: `tests/src/za/mod.rs` (`leak::test_imip(&mut test).await;` after `leak::test_mail` at `:195`)

**Interfaces:**
- Consumes: `scan`, `token_hits`, `stored_pairs`, `hold_task_consumer`, `itip_tasks_for`, `expire_and_purge_blobs`, `smtp_sink`.
- Produces: `pub async fn test_imip(test: &mut TestServer)` (accounts `imip-leak-key`, `imip-leak-key2`).

- [ ] **Step 1: Write the module**

Canaries (added to `EXTRA_CANARIES`): `imipleak-sum-canary`, `imipleak-loc-canary`, `imipleak-desc-canary`, `imipleak-cn-canary`, `imipleak-addr-canary` (an attendee address `imipleak-addr-canary@remote.test`), `imipleak-sender-canary` (the outside organizer `imipleak-sender-canary@remote.test`, and again as the From header's display name), and the outbound set `imipleak-out-sum-canary`, `imipleak-out-att-canary` (`imipleak-out-att-canary@remote.test`). The scan excludes stored-trace records of `Smtp`, `Delivery` and `Queue` events (accepted transit metadata, section 7); if the suite stores no traces, record that.

```rust
let _drain = DrainOn::new();
let held = hold_task_consumer(test).await;
// an outside organizer's invitation to imip-leak-key, delivered and drained; its
// acceptance (an attendee REPLY); a REFRESH from the invitation's attendee canary
// answered; imip-leak-key's own invitation to imip-leak-key2 and the outside
// attendee; its DELETE.
for account in [leak_key, leak_key2] {
    assert_eq!(itip_tasks_for(test, account).await, 0, "a task for a key account");
}
drop(held);
wait_for_queue(test).await;
assert!(queued_recipients(test).await.is_empty());
let sunk = sink().messages();
assert!(sunk.iter().any(|m| m.decoded_parts().iter().any(|p| contains(p, "imipleak-out-sum-canary"))), "positive control");
expire_and_purge_blobs(test, admin).await;
for account in [leak_key, leak_key2] {
    let found = scan(test, account).await;
    assert!(found.violations.is_empty(), "iMIP leaks:\n{}", found.violations.join("\n"));
}
for (_, item) in all_inbox_items(test, [leak_key, leak_key2]).await {
    assert!(matches!(&item.changed_by, ChangedBy::PrincipalId(_)) || item.changed_by == ChangedBy::CalendarAddress(String::new()));
    assert!(!format!("{:?}", item.changed_by).contains("imipleak-sender-canary"));
}
// a pending mail-borne item (one more delivered and left undrained): pk-sealed, no ORGANIZER or ATTENDEE visible
let tokens: Vec<[u8; 16]> = stored_pairs(test, &keys, leak_key, ORGANIZED).await.unwrap().tokens();
assert!(token_hits(test, &tokens).await.is_empty(), "a token in the store");
for message in &sunk {
    for part in message.decoded_parts() {
        assert!(token_forms(&tokens).iter().all(|form| !contains(&part, form)), "a token left in mail");
        assert!(!contains(&part, b"X-ZA-"), "a fork carrier left in mail");
    }
}
```

`token_forms` gives each token in base64url without padding, standard base64 at the three alignments (the token preceded by 0, 1 and 2 bytes, the stable inner characters), and lower and upper hex.

- [ ] **Step 2: Run it, then kill-check it**

Run: the za suite. Expected: pass. Kill check against the branch head commit: `za_imip_item` writes `ChangedBy::CalendarAddress(sender.clone())`; expected catcher: the visible-row assertion; revert, `git status --short crates/` empty.

- [ ] **Step 3: Commit**

```bash
git add tests/src/za/leak.rs tests/src/za/mod.rs
git commit -m "Release 5: the leak scan of mail-borne items, outbound mail and the task queue"
```

### Task 12: Mutation sample, checklist, `CLAUDE.md`, outcome note, full verification

Implementer: `sonnet` (mechanical runs and documentation; the mutation edits are listed exactly).

**Files:**
- Modify: `docs/zero-access/manual-checklist.md` (item 13; a new section after "Mail")
- Modify: `CLAUDE.md`
- Create: `docs/superpowers/plans/2026-11-02-zero-access-plan15-outcome.md`

**Interfaces:**
- Consumes: everything above; each task's report (adjusted assertions, kill lines, spec-question findings).
- Produces: the outcome note later plans read.

- [ ] **Step 1: Mutation runs (spec 8.6)**

Against the branch head commit, one at a time (Global Constraints). Record the first failing assertion (`file:line`, message, left/right) of each, whether or not it is the catcher named; a survivor is a finding for the outcome note and the controller.

| Id | Row of 8.6 | Edit | Expected catcher |
|----|-----------|------|------------------|
| I1 | `!za_key` restored | `ingest.rs` gate gains `&& !za_key` | `imip::inbound::delivered_request`: no item |
| I2 | item without `X-ZA-SENDER` | `za_imip_item` passes `None` as the sender | `delivered_request`: no carrier (then `imip::inbound::replies`: the REPLY rejected) |
| I3 | sender in `changed_by` | `ChangedBy::CalendarAddress(sender.clone())` | `leak::test_imip`: visible rows |
| I4 | no key-account sender refusal | `za_imip_item`'s step 4 removed | `refused_deliveries`: the key account's mail yields an item |
| I5 | authentication check dropped | `sender_authentication.is_authenticated()` removed from the gate | `refused_deliveries`: the no-DMARC sender |
| I6 | known UID without pending items | `za_imip_uid_known` returns the UID index alone | `known_uid_by_pending_item`: no CANCEL item (then `logged_out_request_then_cancel`: ends active) |
| I7 | auto-add not repeated in the drain | `apply_mail`'s auto-add check removed | `contact_deleted_before_drain` |
| I8 | rejected items kept as history | `delete_mail`'s `delete()` replaced by `rewrite_only(tree, None, 0)` | `replies` (the stranger's item still present), `contact_deleted_before_drain` |
| I9 | mail to `ItipMessages::queue` | the create path puts the attempts' contents in `ItipMessages { messages }` and queues it in its batch instead of submitting | `imip::outbound::mailed_once`, `leak::test_imip`: a task with the consumer held |
| I10 | RSVP links on | `services::za_imip` passes `rsvp: true` | `mailed_once`: an RSVP URL in the body |
| I11 | plain local left `NotInternal` | `resolve_inner` returns `NotInternal` for a local non-key account | `statuses`: `5.3` on `imip-out-plain` |
| I12 | key account without a public key mailed | `resolve_inner` returns `Mail` when the served key is missing | `statuses`: `imip-pending` not `5.3` |
| I13a | mail before the commit, create | `za_create_event` submits before its commit | `store_commit_failures` (create): the loser's canary at the sink |
| I13b | same, update | the update path submits before `commit_batch` | `store_commit_failures` (update) |
| I13c | same, DELETE | the DELETE submits before `commit_batch` | `store_commit_failures` (DELETE) |
| I13d | REFRESH before its deletion | the answer submitted before `ctx.delete()` | `imip::outbound::refresh_two_sessions`: two answers at the sink |
| I14 | chunked DELETE mails | the DELETE submits on its error path too | `chunked_collection_delete`: mail at the sink |
| I15 | list left `NotInternal` | `resolve_inner` returns `NotInternal` when `za_account_id_from_email` is `None` | `statuses`: the list's `5.3` |
| I16 | token rule on mail recipients | `route(.., mail: true)` applies release 2's REPLY rule | `attendee_replies`: no REPLY to the outside organizer |
| I17 | delegating merge mails outside delegates | the drain's fan-out passes `internal_only: false` | `outside_delegation`: `1.1` on DELEGATE or a REQUEST at the sink |
| I18 | organizer's-copy gate removed | the gate's condition becomes `true` for `Incoming::Mail` | `delegate_reply_into_delegators_copy`: an item at imip-key2 |
| I19 | `From` traced | `za_imip_item`'s `From = sender.as_str()` | `imip::traces` |

Each runs the za suite.

- [ ] **Step 2: The manual checklist (8.7, 8.5)**

Item 13's last sentence becomes: "Repeat the invitation once with an external address alongside the second account; confirm the external address receives the invitation email and the client shows its `SCHEDULE-STATUS` as `1.1` (sent), while the second account gets no email." Add the section "## Invitations by email (release 5)" after "## Mail", with 8.7's preconditions, recording rules, the eight items verbatim in substance, and its server-side paragraph.

- [ ] **Step 3: `CLAUDE.md`**

- "Where the code lives": `crates/common/src/za/imip.rs` (the submitter seam, `za_imip_submit`, the iMIP trace constants), `crates/services/src/za_imip.rs` (the submitter on upstream's composer), `crates/email/src/message/za/imip.rs` (`za_imip_item`), the mail arm in `drain.rs`, the mail class and `ITIP_STATUS_SENT` in `fanout.rs`, `X-ZA-SENDER` in `seal/item.rs`.
- Invariants: one bullet, "Invitations by email (release 5 spec section 9, 41-47): outbound iMIP is composed in the sender's request and handed to the SMTP queue after its commit, never to the task queue; only the mail class is mailed (`1.1`); a mail-borne item is written only at delivery, sealed to the served key, the sender only in `X-ZA-SENDER`, `changed_by` `CalendarAddress(\"\")`; the drain's mail arm applies upstream's merge without tokens; the fork's iMIP events carry no address or content", and spec 9's amendments.
- The test harness: the hooks `IMIP_SUBMIT_FAIL_ONCE`, `UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_FAIL_ONCE` (`common::za::test_hooks`) and `SECOND_CHUNK_FAIL_ONCE` (`store::za_test`, setter re-exported); the SMTP sink (`tests/src/utils/smtp_sink.rs`, port 12525, the `za-sink` relay route for every non-local domain); `wait_for_queue`.
- The za suite paragraph: `imip::{inbound, outbound, oracle, traces}` after `pinning::test`, `leak::test_imip` after `leak::test_mail`; the `imip-*` fixture allocations of Global Constraints.
- Merge hotspots and silent dependencies: spec 10's lists verbatim in substance, plus: `za_imip_sender` mirrors `ingest.rs:527-536`; `za_imip_mail` reuses `ItipMessages::new`; `compose_imip_message` is `send_imip`'s builder (a change there flows through it); the drain's fill address (spec question 2); `za_imip_uid_known` relies on the UID staying visible on sealed items.
- No release 5 status paragraph: the controller adds it at merge time.

- [ ] **Step 4: Full verification on the branch head**

In order, one at a time: the formatting checks; the per-task compile check; `cargo check -p tests --tests`; `cargo test -p vault -p groupware --features vault/test_mode`; `cargo test -p common za::`; `cargo test -p email --features test_mode za::`; `cargo test -p store --lib --features test_mode za_test`; `cargo test -p services --lib za_imip`; `cargo test -p smtp --lib --bins --features test_mode`; `cargo test -p http@1.0.1 --lib --bins --features test_mode,enterprise`; `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact`; the za suite; the za suite with `BLOB_STORE=FileSystem`; `za::cache_swap::cache_swap_tests -- --exact` (alone, no `BLOB_STORE`); `webdav_tests` plain and key; `imap_tests` and `jmap_tests` plain and key; `cargo test -p tests smtp` (rerun the six known-flaky tests by name); the upstream suites (`webdav::cal_query::`, `store::store_tests`, `store::blob::blob_tests`, `store::lookup::lookup_tests`, `store::search_tests`, `telemetry::telemetry_tests`, `system::system_tests`; `automation::automation_tests` and `directory` need Docker: run them if Docker is up, else record them as left to CI); `python3 -I scripts/tests/ci_steps.py` and `python3 -I scripts/tests/test_gates.py` (no gated step changed); the product build (in the background, no other cargo command alongside; record its time and that the fork's files produced no warning); Task 8's harness runs repeated on the head.
Expected: every one passes, or the known flakes pass on their one rerun.

- [ ] **Step 5: The outcome note**

Create `docs/superpowers/plans/2026-11-02-zero-access-plan15-outcome.md`, in the shape of the plan 13 note:
- **Where things stand**: the branch, every commit by task, each suite's result and time from Step 4.
- **What changed and why**: one paragraph per task, naming the spec sections.
- **Spec questions**: this plan's seven, with what execution found for each.
- **Rulings**: every `Ruling:` line of the ledger, verbatim.
- **Mutation runs (plan 15)**: Step 1's table with the observed lines, plus the kill checks of Tasks 9-11; "No mutation survived" or the exception.
- **Other harnesses**: Task 8's four records.
- **Adjusted assertions**: every existing assertion a task changed, with its reason.
- **Residuals**: spec 5.4's five (a failed submission keeps `1.1`; a local RCPT refusal keeps `1.1`; a crash after the commit loses mail; a collection DELETE's later chunk loses earlier CANCELs; nothing is retried), 4.6's two (non-participant items; a retried delivery's second item), 3.2's thirteen inherited limitations, Jay's decision 11 (issue #72), 5.7's events created before release 5.
- **Deferred findings**: anything a review of this plan deferred; the plan 13 note's open questions, unchanged.
- **Merge hotspots**: spec 10 plus Step 3's additions.

- [ ] **Step 6: Commit**

```bash
git add docs/zero-access/manual-checklist.md CLAUDE.md docs/superpowers/plans/2026-11-02-zero-access-plan15-outcome.md
git commit -m "Plan 15 outcome note, mutation runs, the release 5 checklist, CLAUDE.md"
```
