# Zero-access plan 13: mail encryption at rest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every message stored for a key account is a sealed blob plus a stub metadata archive, written by delivery under the account's public key and by IMAP APPEND, JMAP Email/set and Email/import under the session's mail wrapping key; IMAP and POP3 sessions hold the session keys for the life of the connection and open messages on read; searches, copies, shares, raw blob reads and keyless reads that would need sealed content are refused with a clear error; and traces of a key account's mail carry no content.

**Architecture:**
- **Primitives** (Task 1, `crates/vault`, `crates/groupware/src/calendar/seal/tree.rs`, `crates/email/src/message/za/`): the mail wrapping key MWK = HKDF(MK, `za/v1/mail`) on `SessionKeys`; the groupware envelope and padded-bundle helpers made `pub` with raw-byte variants; the mail sealing module (spec 5): the sealed blob `0x01 || envelope || seal(DEK, mail-blob, raw)`, the stub whose `raw_headers` carries `0x01 || envelope || seal(DEK, mail-meta|blob hash, u32 LE length || rkyv(MessageMetadata) || zero padding to 256)`, the one discriminator (`is_stub`: the format byte at the head of `raw_headers`), and the unsealed view (`open_stub`).
- **Sessions** (Task 2, `crates/imap`, `crates/pop3`): `Session::authenticate` and `handle_auth` call `authenticate_with_keys`, refuse a keyless login to a key account with `NO [NOPERM]` / `-ERR`, and keep the keys on the session's token; `refresh_access_token` re-attaches them (spec 3.3, 4).
- **Writes** (Tasks 3 and 4, `crates/email/src/message/{ingest,copy}.rs`, `crates/http/src/api/vault.rs`, `crates/common/src/auth/vault.rs`): the key-account branch of `email_ingest` (no thread lookup, no threading key, no `MergeThreads`, no duplicate drop, no spam sample, no PGP/S/MIME, a fresh sealed blob, the stub, `thread_id = document_id`, `pk` wrap for a keyless token and `mk` wrap for a token with keys); the provisioning fence (`AssertValue::None` on field 150 in every ordinary ingest and copy batch), the emptiness check extended to mail, the recheck after the pending record, the conditional delete of that record, and the PostgreSQL/MySQL refusal (spec 6.1, 6.6).
- **Reads** (Tasks 5 and 6, `crates/email/src/message/za/view.rs`, IMAP FETCH, POP3 RETR/TOP, Email/get, snippet, submission, `blob_download`, the index task, push, uploads): one `za_metadata_view` helper and one blob opener used at every reader of spec 13 item 1; the visible-field search document; state-only push; sealed uploads and the refusal of raw consumers (spec 5.5, 5.7, 7).
- **Gates and refusals** (Tasks 7 and 8): the JMAP dispatcher gate on key-needing methods, cross-account copy, mailbox sharing and listing, the keyless stub read; the IMAP and JMAP search, sort and snippet refusals (spec 4, 6.3, 8, 9).
- **Traces** (Task 9): whole-session raw I/O redaction on IMAP and POP3, SMTP DATA/BDAT redaction for key recipients and key submitters, `MessageIngest` without the Message-ID, the structured-event floor (milter `Write`/`Read`, spam-filter events, `ImapEvent::Fetch`/`Store`, post-auth parser errors) and the vacation compile reason (spec 10, issue #21).
- **Coverage and docs** (Task 10): key-mode `imap_tests` and `jmap_tests` (`ZA_KEY_ACCOUNTS=1`), the leak-test mail extension, the manual checklist, CLAUDE.md and README-dev, CI, the outcome note.

The fork diff stays narrow: new modules (`email::message::za::{mod,seal,view}`, `jmap::email::za`, the za test modules under `tests/src/za/mail/`, `tests/src/{imap,jmap}/za_variants.rs`) and edits in the fork's own modules (`vault::{keys,session}`, `groupware::calendar::seal::tree`, `common::auth::vault`, `common::za::test_hooks`, `http::api::vault`, `jmap::api::request`'s za helpers). Upstream files change at call sites only, listed per task; the two largest are `email_ingest` (one branch, one assertion) and the readers of spec 13 item 1 (one view call each).

**Tech Stack:** Rust 2024 (Stalwart 0.16.26 fork; the `vault` crate is 0.16.25); x25519-dalek 2, chacha20poly1305 0.11, hkdf 0.13, sha2 0.11, rkyv 0.8.18 (checked decode for the bundle, `rkyv::to_bytes`/`from_bytes`), blake3 (upstream's blob hash), mail-parser; the `tests` crate harness (`TestServer`, `ImapConnection`, `Pop3Connection`, `SmtpConnection` on LMTP 11200, the `jmap_client` Basic client, `HookParked`, the `trc` subscriber pattern of `raw_input.rs`).

**Spec:** `docs/superpowers/specs/2026-10-20-zero-access-mail-design.md`, revision 4 (approved 2026-10-20), cited by section number. Release 1 (`2026-10-06-zero-access-calendar-design.md`), release 2 (`2026-10-11-zero-access-invitations-design.md`) and key pinning (`2026-10-17-zero-access-key-pinning-design.md`) bind where the spec cites them. Project rules: `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, the plan 3 outcome note (gate inventory), the plan 12 outcome note (deferred findings; none is resolved here).

**Spec 13, the open items, as this plan resolves them (each checked in code, HEAD `589e7f02`):**
1. **The stub's readers: twelve direct sites, three without a token.** Direct readers of field 71: IMAP FETCH (`crates/imap/src/op/fetch.rs:360-371`, also reached by SELECT and IDLE with FLAGS-only attribute lists, `select.rs:130`, `idle.rs:243`), POP3 RETR/TOP (`crates/pop3/src/op/fetch.rs:30-43`), Email/get (`crates/jmap/src/email/get.rs:160-177`), SearchSnippet/get (`snippet.rs:140-158`), EmailSubmission/set's `send_message` (`crates/jmap/src/submission/set.rs:553-569`, **no token parameter**; the dispatcher has it at `api/request.rs:504`), `blob_download` (`crates/jmap/src/blob/download.rs:64-79`), `copy_message` (`crates/email/src/message/copy.rs:77-92`, no token, refused for key accounts in Task 7), `add_account_spam_sample` (`ingest.rs:888-904`, no token, gated in Task 3), the index task (`crates/services/src/task_manager/index.rs:454-481`), unindex (`index.rs:603-617`, reads the Subject for the threading clear and `blob_hash`), push (`state_manager/email_push.rs:86-99`, no session) and account destruction (`destroy_account.rs:162-178`, `blob_hash` only). Every indirect reader goes through `blob_download`. The recommendation stands: one `za_metadata_view` helper (Task 5) returning `Cow<'_, Archive<AlignedBytes>>`, built as `unseal_event_archive` builds its view (`crates/groupware/src/calendar/seal/event.rs:341-360`: re-serialize with `Archiver`, `<Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)`, copy `version`); `send_message` and `email_submission_set` gain the token parameter (Task 5). Unindex and destruction read nothing sealed and are unchanged.
2. **Restore re-parses: refused.** `restore_item` (`crates/services/src/task_manager/restore_item.rs:51-69`) reads the archived blob and calls `email_ingest` with `IngestSource::Restore` and `blob_hash: Some(..)`; nothing is restored verbatim. The key-account branch of `email_ingest` refuses `IngestSource::Restore` with a `MessageIngest(Error)` of code 550 (Task 3), which `restore_item.rs:85-96` turns into a permanent task failure. Verbatim restore stays a packaging item (issue #25).
3. **The search document: built in the task.** `build_email_document` (`index.rs:441-486`) gets the two gates and returns `BuildResult::Document` holding `ReceivedAt` (from the stub's `rcvd_attach & MESSAGE_RECEIVED_MASK`) and `Size` (one extra read of `MessageData` at `ValueKey::archive`), through `IndexDocument::new(SearchIndex::Email).with_account_id(..).with_document_id(..)` and `index_unsigned` as `index_document` does (`crates/email/src/message/index/search.rs:43-69`). Ingest's batch stays upstream's (Task 5).
4. **The IMAP refusal reaches the client: confirmed.** A `trc::Error` carrying `Key::Code` is serialized as `<tag> NO [CODE] <details>` (`crates/imap-proto/src/protocol/mod.rs:677-725`); `ResponseCode::NoPerm`, `Cannot` and `ServerBug` exist (`crates/imap-proto/src/lib.rs:126,145,152`). A `SecurityEvent` error is never written (`should_write_err`, `crates/trc/src/event/mod.rs:231-233`) and closes the connection, so the refusal is an `ImapEvent::Error` with `.code(ResponseCode::NoPerm)` plus a separate `trc::event!(Security(Unauthorized), ..)` (Task 2). POP3 has no codes: `Pop3Event::Error` with the fixed text gives `-ERR <text>` (`crates/pop3/src/protocol/response.rs:178-189`).
5. **SMTP DATA redaction: one read site, two flags.** `Session::read` (`crates/smtp/src/inbound/session.rs:543-563`) is the only raw-input site and sees the state (`State::Data`, `Bdat`, `SkipData`) before parsing; a `BDAT n` line and its chunk can share one read in `State::Request`. The session has no account ids for recipients (`SessionAddress`, `crates/smtp/src/core/mod.rs:101-108`; `RcptResolution::Accept` carries none), so RCPT resolution (`rcpt.rs:194-199`) gains one lookup that sets a new `SessionData::za_key_rcpt` flag, cleared with `rcpt_to` (`session.rs:486-489` and after `data.rs:741`). The authenticated sender is known synchronously (`data.authenticated_as.account.is_key_account()`). The read is redacted in `State::Data | Bdat | SkipData` or when the read joined to the partial line holds a `DATA` or `BDAT` command, as `is_auth_input` does for `AUTH` (Task 9).
6. **Push: the conversion point is `push.rs:369-380`.** A subscription without an `email_push` config for the account already receives `email_push.to_state_change()` there; a key account takes the same branch, so a subscription that requested properties gets the state-only payload, and `build_email_push_object` is never reached. It also returns `Ok(None)` for a key account as defence in depth (Task 5).
7. **The `duplicate` Sieve test: hashed, no gate.** `SeenIdHash::new` (`crates/email/src/sieve/mod.rs:83-98`) stores BLAKE3(account id, script hash, id) under `KV_SIEVE_ID`; the Message-ID is never stored. Nothing to gate or list; the outcome note records it.
8. **Threading clear on unindex: harmless, left as is.** `ArchivedMessageMetadata::unindex` (`crates/email/src/message/index/metadata.rs:65-97`) clears the `(account, Email, 90, hash("!"), document_id)` key for a stub (no Subject in the empty root part). The key is per document and was never written; a clear of an absent key is a no-op on RocksDB, FoundationDB and SQLite. No change; the leak test asserts no `Threading` key before and after a delete (Task 10).
9. **Upload coverage: one writer, two raw readers.** `put_jmap_blob` (`crates/common/src/storage/blob.rs:81-136`) is the only function that mints a `Reserved` id for new bytes; its callers are `blob_upload` (`crates/jmap/src/blob/upload.rs:245`) and `blob_upload_many` (`upload.rs:189`). `Blob/copy` re-links an existing hash as `Reserved` (`blob/copy.rs:76-97`). Raw readers of a `Reserved` or `Linked` Email blob besides `blob_download`: the `Blob/upload` data sources (`upload.rs:104,117`), `FileNode/set` create and update (`file/set.rs:108-111, 370-373`) and `FileNode/copy` (`file/copy.rs:188-191`); the purge reads nothing. `/jmap/upload` (`crates/http/src/request.rs:222-270`) performs no membership check at all upstream (Task 6).
10. **Key-mode harness: `create_key_user_account` fits.** `imap_tests` and `jmap_tests` provision users through `admin.create_user_account(name, secret, description, aliases, permissions)` (`tests/src/imap/mod.rs:61-109`, `tests/src/jmap/mod.rs:41-77`); the webdav suite swaps that for `create_key_user_account` with the same arguments under `key_accounts_mode()` (`tests/src/webdav/mod.rs:98-115, 263-267`) and dispatches variants (`:205-247`). Both suites already set `Imap { allow_plain_text_auth: true }`; IMAP logins use `AUTHENTICATE PLAIN` on plain TCP 9991 and JMAP uses Basic over HTTPS, so keys arrive on both. Groups and the admin stay plain. Task 10 sizes the variants from the sub-module flags in its step 1.
11. **The fence's plumbing: three facts.** (a) `deliver_message` maps every error outside `Limit(Quota | TenantQuota)`, `Security(Unauthorized)` and `MessageIngest(Error)` to `TemporaryFailure { reason: "Transient server failure." }` (`crates/email/src/message/delivery.rs:232-267`), `StoreEvent::AssertValueFailed` included. (b) `Store::is_pg_or_mysql()` exists (`crates/store/src/lib.rs:652-725`); `setup-token` reads it through `server.store()`. (c) No delete helper exists (`za_vault_write`, `za_vault_commit` and `vault_commit_store` are the only writers); Task 4 adds `Server::za_vault_delete(account_id, previous: &VaultRead)` as one more conditional write through `vault_batch_write`'s pattern (`crates/common/src/auth/vault.rs:163-196`), asserting `AssertValue::Hash(previous.cas)` and clearing field 150, which is what the test precedent does unconditionally (`tests/src/za/setup.rs:178-183`).
12. **The structured-event inventory** is in Task 9, step 1, with the decision per site: `[redacted]` keeps the event, as recommended.

**Decisions this plan makes under spec 13's preamble (Jay can override any at plan review):**
- The inner (sealed) metadata archive's `blob_hash` names the **sealed** blob, the same hash the stub carries, so the unsealed view's `blob_hash` is what readers fetch and the `mail-meta` associated data binds the stub to that blob.
- Cross-account `Email/copy` and `Blob/copy` by or into a key account answer the **method error** `forbidden` (spec 6.3 says "a JMAP `forbidden` set error"); the whole call is invalid for that account pair, and the dispatcher gate of spec 4 already answers at the method level.
- Shared-mailbox listing exclusion lives in `build_access_token` (`crates/common/src/auth/access_token.rs:77-125`): `Collection::Mailbox` and `Collection::Email` are dropped from an `access_to` entry whose granting account is a key account. Every consumer (IMAP `shared_accounts`, the JMAP session object, `Mailbox/get`, `Email/query`'s mask, `has_access_blob`) then sees no access, and the per-reader refusal of spec 6.3 stays as defence in depth for a stale ACL read straight from the mailbox record.
- `ImapEvent::Store`'s `Details` (keyword names) is redacted in a key session with `ImapEvent::Fetch`'s, as spec 10 lists it, although keyword names are visible by structure; mailbox names in `MailboxName` keys are routing structure and stay.
- The spam-filter floor: `SpamFilterInput` gains `za_redact: bool`; with it set the DNSBL `Hostname`, the LLM `Details` and the `Eval` result events (through `za_eval_if`) are withheld, since each can carry a message-derived string. `Spam(Classify)`'s recipient list is transit metadata and stays.
- The restore refusal and the "no served key" delivery failure are the ingest branch's own errors, so every caller of `email_ingest` sees one behaviour.

**Execution:** subagent-driven development on a new branch `release3-mail` cut from `release3-mail-spec` (the spec at `589e7f02`, this plan committed on top); implementers run at `opus` (kernel work: sealing, ingest, the fence, the readers), reviewers at the session's model ceiling (`fable` today); ledger `.superpowers/sdd/2026-10-20-zero-access-13-mail/progress.md`.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build with `enterprise`.
- Per-task compile check, shipping configuration; the build must succeed (exit 0) before the warning filter runs, so a failed build is never read as `clean`:
  ```bash
  log="${TMPDIR:-/tmp}/za-check.log"
  cargo check -p stalwart --no-default-features --features rocks > "$log" 2>&1 || { tail -40 "$log"; echo BUILD FAILED; false; }
  grep -E "^(warning|error)" -A3 "$log" | grep -E "crates/(vault|groupware|email|imap|pop3|smtp|spam-filter|jmap|services)/|crates/http/src/(request.rs|api/|auth/)|crates/common/src/(za/|auth/|storage/blob.rs)" || echo clean
  ```
  Upstream's known warnings in store, common, jmap and services are not this plan's.
- Test-mode compile check: `cargo check -p tests --tests`.
- Unit tests: `cargo test -p vault`, `cargo test -p groupware`, `cargo test -p email --features test_mode za::`, `cargo test -p common auth::vault`, `cargo test -p common za::`.
- Integration: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`; at the end also with `BLOB_STORE=FileSystem`. Key-mode mail suites: the same prefix with `ZA_KEY_ACCOUNTS=1` and `imap_tests` or `jmap_tests`. CalDAV: `webdav_tests`, plain and with `ZA_KEY_ACCOUNTS=1`; upstream's `cal_itip` sub-test is timing-flaky, rerun once before calling a failure there a regression.
- One cargo command at a time; never while Jay's manual server (ports 8080/8443/1143) is under test.
- Formatting: `cargo fmt -p <crate> -- --check` for each touched crate other than http (`vault`, `groupware`, `email`, `imap`, `pop3`, `smtp`, `spam-filter`, `jmap`, `services`, `common`, `tests`), and `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` for http. Build output warning-free for new code.

Spec values, verbatim:
- MWK label `za/v1/mail` (HKDF-SHA256 over MK, 32 bytes, as `derive_event_key` derives the EWK with `za/v1/events`).
- Blob: `0x01 || envelope || seal(DEK, aad_blob, raw)`; the format byte is `MAIL_FORMAT_V1 = 1`; `seal` is `vault::keys::seal` (24-byte nonce, XChaCha20-Poly1305, 16-byte tag). Nothing padded.
- Envelope: `mk` = `[POLICY_VERSION (1), WRAP_MK (1), wrapped DEK (72)]`, wrapped under the MWK; `pk` = `[POLICY_VERSION (1), WRAP_PK (2), ephemeral public key (32), wrapped DEK (72)]`, the `pk_wrap` of `vault::keys` with label `za/v1/pk`. A reader refuses any policy byte other than 1 and any wrap byte other than 1 or 2.
- Stub: `contents` = one `MessageMetadataContents` with empty `html_body`, `text_body`, `attachments` and one `MessageMetadataPart { headers: [], body: MetadataPartType::Binary, flags: 0, offset_header: 0, offset_body: 0, offset_end: 0 }`; `rcvd_attach` = the arrival time with the attachment bit clear; `blob_hash` = the sealed blob's hash; `blob_body_offset: 0`; `preview: ""`; `raw_headers` = `0x01 || envelope || seal(DEK, aad_meta, u32 LE length || rkyv(MessageMetadata) || zero padding to a multiple of 256)`.
- Associated data: `aad("mail-key", account_id)` for the envelope, `aad("mail-blob", account_id)` for the blob, `aad("mail-meta", account_id) || b'|' || blob hash (32 bytes)` for the bundle (`vault::keys::aad` builds `za/v1|<purpose>|<account id, 4 bytes big-endian>`).
- `MessageData.size` = the plaintext size as upstream computes it; `thread_id` = the message's own document id; no `Threading` key, no `MergeThreads`, no duplicate drop, no spam sample, no PGP/S/MIME, no iMIP.
- The visible-field search document: `ReceivedAt` and `Size`, no terms.
- Fixed strings: trace details `zero-access: mail login without session keys`, `zero-access: mail access without session keys`, `zero-access: mail copy across accounts`, `zero-access: sharing refused on a key account's mailbox`, `zero-access: raw read of a sealed source`, `zero-access: restore of a key account's message`, `zero-access: delivery without a served public key`, `zero-access: unreadable message` (the `DataCorruption` reason); client texts `ZA_MAIL_LOGIN_REFUSED = "Zero-access accounts log in to mail with the password or an app password."`, `ZA_MAIL_KEYLESS = "Mail in this account is encrypted at rest and is not available to this session."`, `ZA_MAIL_SEARCH_REFUSED = "Mail in this account is encrypted at rest; the server cannot search message content. Search your local copy instead."`, `ZA_MAIL_UNREADABLE = "Message could not be read."`; 409 reasons `account already holds mail`, `key accounts are not supported on PostgreSQL or MySQL`.
- All of these are constants: no setting, permission, event type or environment knob (release 1 section 1.4); no edit of generated code (`crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`).

Invariants and code rules:
- Release 1's invariants 1-10, release 2's 11-15 and key pinning's 16-23 hold with spec 12's two amendments; spec 12's 24-32 are added. In short: every message of a key account is a stub plus a sealed blob whatever wrote it (24); the stub carries the arrival time, the blob hash, the bundle and the empty root part only (25); a mail session of a key account exists only with keys and every read holds the owner's keys or is refused (26); no threading index, full-text term, spam sample or push payload with content (27); plaintext outside process memory only in the queue and the staged blob of a failed ordinary ingest (28); delivery never reads a session key and never stores in the clear (29); traces carry no content (30); no raw read of a sealed source (31); the ordinary-writer assertion and the vacation reason are invariant 9 exceptions (32).
- No stored struct changes layout: `MessageMetadata`, `MessageData`, `MessageMetadataContents`, `MessageMetadataPart` keep theirs; ciphertext rides in `raw_headers` and the blob.
- Non-key accounts take unchanged paths (invariant 9), except the one assertion op in ordinary ingest and copy batches and the vacation compile reason.
- `SessionKeys` are attached to a mail session's token by `Session::authenticate` / `handle_auth` only; `refresh_access_token` re-attaches the session's own; WebSocket, EventSource, ManageSieve and SMTP never hold keys. Background code never needs a key; the index task, unindex, push, destruction and the purge read nothing sealed.
- Key material and decrypted plaintext live in `Zeroizing` buffers; no key, DEK, fingerprint, header, body text or search term in traces or errors. A view archive is never written back, never passed to a write as the current value, and never cached.
- Every vault write goes through `Server::za_vault_commit`, `za_commit` or the new `za_vault_delete`; never `za_vault_write` directly.
- Traces: refusals are `trc::event!(Security(SecurityEvent::Unauthorized), AccountId = .., Details = "zero-access: ...")` plus the client-facing error; unreadable messages are `trc::error!(StoreEvent::DataCorruption ... AccountId, DocumentId, Reason = "zero-access: unreadable message")` through `groupware::calendar::seal::seal_error`'s shape; the delivery failure is `trc::error!` with `AccountId` and the fixed reason.
- Test addresses: `key1@example.com` (alice) to `key6@example.com` exist from `setup::test`; new accounts under `tests/src/za/mail/` use `keymail<n>@example.com`; plain controls `plain@example.com` and new `plainmail<n>@example.com`; never `.example`.
- Commit messages: subject, blank line, then exactly `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_017qWFjNmU7xZ8ro8BdKrPpu`.
- Never open issues, pull requests or reports against upstream `stalwartlabs/stalwart`; upstream bugs found here go in the outcome note only.

Tests:
- The za suite skips every drain (`DRAIN_SKIP_ALL`); the mail modules do not touch the drain. New modules live in `tests/src/za/mail/` and run after `index_task::test_sealed_archive` and before `destroy_key_accounts`, in the order Task 10 fixes; each `pub async fn test(test: &mut TestServer)` is self-contained: it creates the accounts it names with `create_key_user_account`, deletes its messages (Email/set destroy or `destroy_all_mailboxes_for_account`) and mailboxes, revokes its app passwords, and leaves no queued mail (`wait_for_delivery`).
- Mail reaches a key account through LMTP on 11200 (`SmtpConnection::connect().ingest(from, &[to], message)`, `tests/src/utils/smtp.rs:37`), which runs the real queue and `deliver_message`; `wait_for_delivery` then waits for the queue and the index tasks. Submission uses the za suite's implicit-TLS listener on 4465.
- IMAP clients: `ImapConnection::connect(tag)` (plain 9991, `AUTHENTICATE PLAIN`); the raw-input `Conn` (TLS 9992) only where TLS matters. POP3: `Pop3Connection::connect()` (TLS 4110). JMAP: `account.jmap_client()` (Basic) and `account.jmap_method_call(..)` for raw method calls; a Bearer token comes from the OAuth flow the `tracing` module already drives.
- New hooks are cleared at the end of the step that set them. Kill checks and mutation runs are made against a commit, one at a time, each reverted with `git checkout -- <file>` and followed by an empty `git status --short crates/`; none is committed.
- An existing za, imap or jmap assertion that fails only because of a designed refusal is adjusted with a comment citing the spec section and listed in the task report; any other failure is a regression.

## Review Focus

Inputs the spec implies that no case of section 11 exercises, most likely to bite first. Each has its test in the owning task.

1. **IDLE delivery in a key session.** SELECT and IDLE load every message's metadata archive with FLAGS-only attribute lists (`select.rs:130`, `idle.rs:243`); a stub reached there must open through the view, including the first `pk` open of the session inside the IDLE task. Test: a key account's IMAP session in IDLE receives a delivered message (`* n EXISTS`), ends IDLE, and FETCHes it. Task 5, `rf_1_idle_delivery`.
2. **A stale ACL grant made before provisioning.** A plain account shares its INBOX with `AddItems` and `Read`, then becomes a key account (setup refuses mail, not grants). The grantee's `access_to` no longer lists it, but `check_mailbox_acl` and the cross-account APPEND path read the mailbox record's ACL directly (`append.rs:82-115`, `core/mailbox.rs:402-437`). Test: the grantee's SELECT of the shared mailbox and APPEND into it answer `NO [NOPERM]`, and the key account's mailbox is absent from the grantee's LIST and `Mailbox/get`. Task 7, `rf_2_stale_grant`.
3. **A key account writes into a plain account's shared mailbox.** The appender's token carries keys for its own account only; the target is plain, so ingest must take the plain path and store plaintext the owner can read. Test: a plain account grants `AddItems` to a key account; the key account APPENDs there; the owner FETCHes the body. Task 3, `rf_3_append_into_plain_share`.
4. **Password change while connected.** `POST /api/vault/password` during an open IMAP session: the session's FETCH keeps working (keys follow the connection, spec 4), a new login with the old password is refused and with the new password succeeds and reads the same message (the `pk`-sealed message opens with the unchanged private key). Task 2, `rf_4_password_change_mid_session`.
5. **A stub past the archive compression watermark.** A message with a 20 KiB header block gives a bundle above `COMPRESS_WATERMARK` (8192), so the stub's archive is LZ4-compressed by `Archiver` and the view's decode must handle the compressed form. Test: the message round-trips through FETCH `BODY[HEADER]` and Email/get `headers`. Task 5, `rf_5_large_header_block`.

## Case map

| Spec 11 item | Task |
|---|---|
| Unit: MWK derivation; blob and stub round-trips under both wraps; the stub's visible fields; bundle length a multiple of 256; tampered blob, tampered bundle, bundle moved to another blob hash, envelope moved to another account, policy byte, unknown wrap; plaintext archive detected | 1 |
| Unit: the unsealed view's offsets locate every part of a corpus | 5 |
| Login with the password and an app password; OAUTHBEARER, XOAUTH2, master user refused; TOTP over IMAP; keys gone after LOGOUT, UNAUTHENTICATE and a dropped connection | 2 |
| Delivery to a pending account; delivery through Sieve (`fileinto`, `vacation`, `redirect`); the same message to a key and a plain account; APPEND, Email/set and Email/import under `mk`; COPY within the account | 3 |
| The provisioning fence (forward, Sieve-modified, reverse, `copy_message`), the stress test, `setup-token` refused on PostgreSQL and MySQL | 4 |
| Offsets round-trip; a corrupted stub and a corrupted blob; a plaintext message served as plaintext and the classification test; threads as singletons; `EmailSubmission` with Bcc stripped; push state-only | 5 |
| Uploads refused and accepted; a sealed upload imported and attached; raw consumers refused | 6 |
| The cross-account, sharing and keyless-token gates | 7 |
| Every IMAP SEARCH and SORT key and every JMAP filter and sort on both sides of the line; snippets; the visible-field document's content | 8 |
| Traces: milter events, FETCH `HEADER.FIELDS`, an authenticated malformed command, against a key and a plain account | 9 |
| Key-mode `imap_tests` and `jmap_tests`; the leak extension; the manual checklist | 10 |

---

### Task 1: Mail sealing primitives

**Files:**
- Modify: `crates/vault/src/keys.rs` (label constant after `LABEL_PK`, line 28; `derive_mail_key` after `derive_event_key`, line 150; the `pk_unwrap` doc comment, lines 380-381; tests in `mod tests`)
- Modify: `crates/vault/src/session.rs` (`SessionKeys` fields, lines 18-30; `new`, lines 140-150; accessor after `ewk()`, line 156; the `test_mode` counter)
- Modify: `crates/vault/Cargo.toml` (`[features] test_mode = []`), `tests/Cargo.toml:44` (`vault = { path = "../crates/vault", features = ["test_mode"] }`)
- Modify: `crates/groupware/src/calendar/seal/tree.rs` (visibility of `FORMAT_V1`, `WRAP_MK`, `WRAP_PK`, `PAD`, `seal_key_envelope`, `open_key_envelope`, `seal_pk_envelope`, `open_pk_envelope`, `decode_archive`; the split of `seal_bytes_as` / `open_bytes_as` into raw-byte cores), `crates/groupware/src/calendar/seal/mod.rs:29` (the `pub use tree::{..}` line)
- Modify: `crates/email/Cargo.toml` (`vault = { path = "../vault" }`, `zeroize`), `crates/email/src/message/mod.rs` (`pub mod za;`)
- Create: `crates/email/src/message/za/mod.rs`, `crates/email/src/message/za/seal.rs`
- Test: unit tests in `keys.rs`, `session.rs`, `tree.rs`, `za/seal.rs`

**Interfaces:**
- Consumes: `vault::keys::{Secret, KeyError, aad, seal, open, wrap_key, unwrap_key, pk_wrap, pk_unwrap, PUBLIC_KEY_LEN, WRAP_LEN, NONCE_LEN, TAG_LEN}`; `vault::session::SessionKeys`; `groupware::calendar::seal::policy::POLICY_VERSION`; `email::message::metadata::{MessageMetadata, ArchivedMessageMetadata, MessageMetadataContents, MessageMetadataPart, MetadataPartType, MESSAGE_HAS_ATTACHMENT, MESSAGE_RECEIVED_MASK}`.
- Produces, in `vault::keys`: `const LABEL_MAIL: &[u8] = b"za/v1/mail";` and `pub fn derive_mail_key(mk: &Secret) -> Secret` (`hkdf_expand(mk.as_bytes(), LABEL_MAIL)`). The `pk_unwrap` doc comment becomes "The drain and a mail session holding the owner's keys call this (invariant 11 as amended by the mail spec, section 12)."
- Produces, in `vault::session`: the field `mwk: Secret` on `SessionKeys`, derived in `new` next to `ewk`; `pub fn mwk(&self) -> &Secret`; under `#[cfg(feature = "test_mode")]` a `pub mod live` with `pub fn session_keys(account_id: u32) -> usize` backed by a `Mutex<HashMap<u32, usize>>` that `SessionKeys::new` increments and a `Drop for SessionKeys` decrements (the `Drop` exists only under the feature; the secrets zero themselves through `Zeroizing`).
- Produces, in `groupware::calendar::seal::tree`, now `pub`: `FORMAT_V1`, `WRAP_MK`, `WRAP_PK`, `PAD`, `seal_key_envelope(dek, wrapping: &Secret, aad) -> Vec<u8>` (the second parameter renamed from `ewk` to `wrapping`; the EWK call sites pass `keys.ewk()` unchanged), `open_key_envelope(envelope, wrapping, aad) -> Result<Secret, SealError>`, `seal_pk_envelope`, `open_pk_envelope`, `decode_archive<T>`, and the two new raw cores `pub fn seal_padded(dek: &Secret, aad: &[u8], plaintext: &[u8]) -> Vec<u8>` (u32 LE length prefix, plaintext, zero padding to a multiple of `PAD`, then `seal`; no format byte, no base64) and `pub fn open_padded(dek: &Secret, aad: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, SealError>` (`open`, then the length prefix, `SealError::Format` on a short or inconsistent buffer). `seal_bytes_as` becomes `base64(format || seal_padded(dek, format_aad(format, aad), plaintext))` and `open_bytes_as` the inverse; their outputs are byte for byte what they were (the existing tree tests pin that). Re-exported from `seal/mod.rs`: `SealError, seal_error, tree_has_carriers, decode_archive, seal_padded, open_padded, seal_key_envelope, open_key_envelope, seal_pk_envelope, open_pk_envelope, FORMAT_V1, WRAP_MK, WRAP_PK, PAD`.
- Produces, in `email::message::za` (`mod.rs` re-exports `seal::*` and holds the constants):
  - `pub const MAIL_FORMAT_V1: u8 = 1;`, `pub const AAD_MAIL_KEY: &str = "mail-key";`, `pub const AAD_MAIL_BLOB: &str = "mail-blob";`, `pub const AAD_MAIL_META: &str = "mail-meta";`
  - `pub fn mail_key_aad(account_id: u32) -> Vec<u8>`, `pub fn mail_blob_aad(account_id: u32) -> Vec<u8>` (both `aad(purpose, account_id)`), `pub fn mail_meta_aad(account_id: u32, blob_hash: &BlobHash) -> Vec<u8>` (`aad(AAD_MAIL_META, account_id) || b'|' || blob_hash.as_slice()`).
  - `pub enum MailWrap<'a> { Session(&'a SessionKeys), Public(&'a [u8; PUBLIC_KEY_LEN]) }`.
  - `pub fn seal_mail_envelope(dek: &Secret, wrap: MailWrap<'_>, account_id: u32) -> Result<Vec<u8>, SealError>`: `Session` gives `seal_key_envelope(dek, keys.mwk(), &mail_key_aad(account_id))`, `Public` gives `seal_pk_envelope(dek, public, &mail_key_aad(account_id))`.
  - `pub fn envelope_len(bytes: &[u8]) -> Result<usize, SealError>`: `bytes` starts at the policy byte; `[POLICY_VERSION, WRAP_MK, ..]` is `2 + WRAP_LEN`, `[POLICY_VERSION, WRAP_PK, ..]` is `2 + PUBLIC_KEY_LEN + WRAP_LEN`, another policy byte `SealError::Policy(b)`, anything else `SealError::Format`; a buffer shorter than the length is `SealError::Format`.
  - `pub fn envelope_needs_private_key(bytes: &[u8]) -> Result<bool, SealError>` (the wrap byte is `WRAP_PK`).
  - `pub fn open_mail_envelope(envelope: &[u8], keys: &SessionKeys, private_key: Option<&Secret>, account_id: u32) -> Result<Secret, SealError>`: `mk` through `open_key_envelope(.., keys.mwk(), ..)`; `pk` through `open_pk_envelope(.., private_key, ..)`, and `Err(SealError::Structure("private key"))` when `private_key` is `None`.
  - `pub fn seal_mail_blob(dek: &Secret, envelope: &[u8], account_id: u32, raw: &[u8]) -> Vec<u8>` = `[MAIL_FORMAT_V1] || envelope || seal(dek, &mail_blob_aad(account_id), raw)`.
  - `pub fn split_sealed(bytes: &[u8]) -> Result<(&[u8], &[u8]), SealError>`: checks the format byte (`SealError::Format` otherwise, `SealError::NotSealed` for an empty buffer), returns `(envelope, sealed)`.
  - `pub fn open_mail_blob(sealed: &[u8], keys: &SessionKeys, private_key: Option<&Secret>, account_id: u32) -> Result<Zeroizing<Vec<u8>>, SealError>`: `split_sealed`, `open_mail_envelope`, `open(dek, &mail_blob_aad(account_id), ..)`.
  - `pub fn stub_metadata(full: &MessageMetadata, bundle: Vec<u8>) -> MessageMetadata`: the stub of Global Constraints with `rcvd_attach = full.rcvd_attach & MESSAGE_RECEIVED_MASK` and `blob_hash = full.blob_hash.clone()`.
  - `pub fn seal_stub(full: MessageMetadata, dek: &Secret, envelope: &[u8], account_id: u32) -> Result<MessageMetadata, SealError>`: `rkyv::to_bytes::<rkyv::rancor::Error>(&full)` (`SealError::Format` on error), `bundle = [MAIL_FORMAT_V1] || envelope || seal_padded(dek, &mail_meta_aad(account_id, &full.blob_hash), &bytes)`, then `stub_metadata(&full, bundle)`; the serialized plaintext is a `Zeroizing` buffer.
  - `pub fn is_stub(metadata: &ArchivedMessageMetadata) -> bool` = `metadata.raw_headers.first() == Some(&MAIL_FORMAT_V1)`.
  - `pub fn open_stub(stub: &ArchivedMessageMetadata, keys: &SessionKeys, private_key: Option<&Secret>, account_id: u32) -> Result<MessageMetadata, SealError>`: `split_sealed(&stub.raw_headers)` (`SealError::NotSealed` when `!is_stub`), `envelope_len`, `open_mail_envelope`, `open_padded(dek, &mail_meta_aad(account_id, &BlobHash::from(&stub.blob_hash)), bundle)`, `decode_archive::<MessageMetadata>`, then `rcvd_attach = (inner.rcvd_attach & MESSAGE_HAS_ATTACHMENT) | (stub.rcvd_attach & MESSAGE_RECEIVED_MASK)` (spec 7: the stub's time bits are authoritative).
  - `pub fn stub_needs_private_key(stub: &ArchivedMessageMetadata) -> Result<bool, SealError>` and `pub fn blob_needs_private_key(sealed: &[u8]) -> Result<bool, SealError>` (both through `split_sealed` and `envelope_needs_private_key`).
  - `pub fn new_dek() -> Secret` (`Secret::random()`).

- [ ] **Step 1: Write the failing unit tests in `vault` (`keys.rs`, `session.rs`)**

```rust
#[test]
fn the_mail_key_is_deterministic_and_independent_of_the_event_key() {
    let mk = Secret::random();
    assert_eq!(derive_mail_key(&mk).as_bytes(), derive_mail_key(&mk).as_bytes());
    assert_ne!(derive_mail_key(&mk).as_bytes(), derive_event_key(&mk).as_bytes());
    assert_ne!(derive_mail_key(&mk).as_bytes(), mk.as_bytes());
    assert_ne!(derive_mail_key(&mk).as_bytes(), derive_mail_key(&Secret::random()).as_bytes());
}

#[test]
fn session_keys_carry_the_mail_key() {
    let mk = Secret::random();
    let keys = SessionKeys::new(7, 3, mk.clone());
    assert_eq!(keys.mwk().as_bytes(), derive_mail_key(&mk).as_bytes());
    assert_eq!(format!("{keys:?}"), "SessionKeys(account 7, generation 3)");
}

#[cfg(feature = "test_mode")]
#[test]
fn the_live_counter_follows_construction_and_drop() {
    let before = live::session_keys(99);
    let keys = std::sync::Arc::new(SessionKeys::new(99, 1, Secret::random()));
    let clone = keys.clone();
    assert_eq!(live::session_keys(99), before + 1);
    drop(keys);
    assert_eq!(live::session_keys(99), before + 1, "a clone keeps the keys alive");
    drop(clone);
    assert_eq!(live::session_keys(99), before);
}
```

- [ ] **Step 2: Write the failing unit tests in `tree.rs` for the raw cores**

```rust
#[test]
fn seal_padded_is_the_inside_of_seal_bytes() {
    let (dek, aad) = (Secret::random(), b"za/v1|t|x".to_vec());
    let raw = seal_padded(&dek, &aad, b"hello");
    assert_eq!(raw.len(), NONCE_LEN + 256 + TAG_LEN);
    assert_eq!(open_padded(&dek, &aad, &raw).unwrap().as_slice(), b"hello");
    let big = vec![7u8; 300];
    assert_eq!(seal_padded(&dek, &aad, &big).len(), NONCE_LEN + 512 + TAG_LEN);
    assert!(matches!(open_padded(&dek, b"other", &raw), Err(SealError::Aead)));
    assert!(matches!(open_padded(&dek, &aad, &raw[..raw.len() - 1]), Err(SealError::Aead)));
    // The base64 wrapper still round-trips through the core.
    assert_eq!(open_bytes(&dek, &aad, &seal_bytes(&dek, &aad, b"hello")).unwrap().as_slice(), b"hello");
}
```

- [ ] **Step 3: Write the failing unit tests in `crates/email/src/message/za/seal.rs`**

`sample_metadata(blob_hash)` is a local helper building a two-part `MessageMetadata` (a root multipart with a text part and a binary attachment part, a Subject and From header, `preview: "preview canary"`, `raw_headers: b"Subject: x\r\n\r\n"`, `blob_body_offset: 14`, `rcvd_attach: MESSAGE_HAS_ATTACHMENT | 1_700_000_000`), with `build_metadata_contents` over a parsed message or by hand.

```rust
#[test]
fn a_blob_round_trips_under_both_wraps_and_binds_the_account() {
    let keys = SessionKeys::new(7, 1, Secret::random());
    let (public, private) = generate_keypair();
    for wrap in [MailWrap::Session(&keys), MailWrap::Public(&public)] {
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, wrap, 7).unwrap();
        let sealed = seal_mail_blob(&dek, &envelope, 7, b"raw message");
        assert_eq!(sealed[0], MAIL_FORMAT_V1);
        assert_eq!(envelope_len(&sealed[1..]).unwrap(), envelope.len());
        let opened = open_mail_blob(&sealed, &keys, Some(&private), 7).unwrap();
        assert_eq!(opened.as_slice(), b"raw message");
        assert!(matches!(open_mail_blob(&sealed, &keys, Some(&private), 8), Err(SealError::Aead)));
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(matches!(open_mail_blob(&tampered, &keys, Some(&private), 7), Err(SealError::Aead)));
    }
    let dek = new_dek();
    let pk_sealed = seal_mail_blob(&dek, &seal_mail_envelope(&dek, MailWrap::Public(&public), 7).unwrap(), 7, b"x");
    assert!(blob_needs_private_key(&pk_sealed).unwrap());
    assert!(matches!(open_mail_blob(&pk_sealed, &keys, None, 7), Err(SealError::Structure("private key"))));
    let other = SessionKeys::new(7, 1, Secret::random());
    let mk_sealed = seal_mail_blob(&dek, &seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap(), 7, b"x");
    assert!(!blob_needs_private_key(&mk_sealed).unwrap());
    assert!(matches!(open_mail_blob(&mk_sealed, &other, None, 7), Err(SealError::Aead)));
}

#[test]
fn a_stub_carries_only_the_visible_fields_and_round_trips() {
    let keys = SessionKeys::new(7, 1, Secret::random());
    let hash = BlobHash::generate(b"sealed bytes");
    let full = sample_metadata(hash.clone());
    let dek = new_dek();
    let envelope = seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap();
    let stub = seal_stub(full.clone(), &dek, &envelope, 7).unwrap();
    assert_eq!(stub.contents.len(), 1);
    assert_eq!(stub.contents[0].parts.len(), 1);
    let part = &stub.contents[0].parts[0];
    assert!(part.headers.is_empty() && matches!(part.body, MetadataPartType::Binary));
    assert_eq!((part.flags, part.offset_header, part.offset_body, part.offset_end), (0, 0, 0, 0));
    assert!(stub.contents[0].html_body.is_empty() && stub.contents[0].text_body.is_empty() && stub.contents[0].attachments.is_empty());
    assert_eq!(stub.rcvd_attach, 1_700_000_000);
    assert_eq!(stub.blob_hash, hash);
    assert_eq!(stub.blob_body_offset, 0);
    assert!(stub.preview.is_empty());
    assert_eq!(stub.raw_headers[0], MAIL_FORMAT_V1);
    let bundle_len = stub.raw_headers.len() - 1 - envelope.len() - NONCE_LEN - TAG_LEN;
    assert_eq!(bundle_len % 256, 0);
    let archived = archive(&stub); // Archiver::new(stub).serialize(), then unarchive: a local helper
    assert!(is_stub(&archived));
    let opened = open_stub(&archived, &keys, None, 7).unwrap();
    assert_eq!(opened.raw_headers, full.raw_headers);
    assert_eq!(opened.preview, full.preview);
    assert_eq!(opened.blob_body_offset, full.blob_body_offset);
    assert_eq!(opened.rcvd_attach, full.rcvd_attach);
    assert_eq!(opened.contents.len(), full.contents.len());
    assert_eq!(opened.contents[0].parts.len(), full.contents[0].parts.len());
}

#[test]
fn a_stub_refuses_a_moved_bundle_a_bad_policy_and_an_unknown_wrap() {
    let keys = SessionKeys::new(7, 1, Secret::random());
    let dek = new_dek();
    let envelope = seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap();
    let stub = seal_stub(sample_metadata(BlobHash::generate(b"a")), &dek, &envelope, 7).unwrap();
    let mut moved = stub.clone();
    moved.blob_hash = BlobHash::generate(b"b");
    assert!(matches!(open_stub(&archive(&moved), &keys, None, 7), Err(SealError::Aead)), "a bundle moved to another blob");
    assert!(matches!(open_stub(&archive(&stub), &keys, None, 8), Err(SealError::Aead)), "an envelope moved to another account");
    let mut tampered = stub.clone();
    let last = tampered.raw_headers.len() - 1;
    tampered.raw_headers[last] ^= 1;
    assert!(matches!(open_stub(&archive(&tampered), &keys, None, 7), Err(SealError::Aead)));
    let mut policy = stub.clone();
    policy.raw_headers[1] = 2;
    assert!(matches!(open_stub(&archive(&policy), &keys, None, 7), Err(SealError::Policy(2))));
    let mut wrap = stub.clone();
    wrap.raw_headers[2] = 9;
    assert!(matches!(open_stub(&archive(&wrap), &keys, None, 7), Err(SealError::Format)));
}

#[test]
fn a_plaintext_archive_is_not_a_stub() {
    let full = sample_metadata(BlobHash::generate(b"a"));
    let archived = archive(&full);
    assert!(!is_stub(&archived));
    assert!(matches!(open_stub(&archived, &SessionKeys::new(7, 1, Secret::random()), None, 7), Err(SealError::NotSealed)));
    let mut crafted = full.clone();
    crafted.raw_headers = vec![MAIL_FORMAT_V1, 1, 1].into_boxed_slice();
    assert!(is_stub(&archive(&crafted)), "a header block starting with the format byte reads as a stub");
    assert!(matches!(open_stub(&archive(&crafted), &SessionKeys::new(7, 1, Secret::random()), None, 7), Err(SealError::Format)));
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p vault`, `cargo test -p groupware seal::tree`, `cargo test -p email za::`
Expected: compile errors (`derive_mail_key`, `mwk`, `seal_padded`, `za` not found).

- [ ] **Step 5: Implement the vault side**

`derive_mail_key` and `LABEL_MAIL` in `keys.rs`; the `mwk` field, its derivation in `new`, `mwk()`, the `test_mode` counter module and `Drop` in `session.rs`; the `test_mode` feature in `crates/vault/Cargo.toml` and its enablement in `tests/Cargo.toml`; the `pk_unwrap` doc comment.

- [ ] **Step 6: Implement the groupware side**

Widen the visibilities, rename the `ewk` parameter to `wrapping`, split `seal_bytes_as` / `open_bytes_as` around `seal_padded` / `open_padded`, extend the `pub use` in `seal/mod.rs`. The existing tree, event, collection and item tests must pass unchanged.

- [ ] **Step 7: Implement `crates/email/src/message/za/{mod,seal}.rs`**

Add `vault` and `zeroize` to `crates/email/Cargo.toml`; `pub mod za;` in `message/mod.rs`. The functions of the Interfaces block, with the SPDX header copied from a neighbour; the plaintext of `seal_stub` and the output of `open_stub`'s `open_padded` in `Zeroizing` buffers.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p vault`, `cargo test -p vault --features test_mode`, `cargo test -p groupware`, `cargo test -p email za::`; then the compile check and `cargo fmt` for `vault`, `groupware`, `email`.
Expected: all pass, `clean`.

- [ ] **Step 9: Commit**

```bash
git add crates/vault crates/groupware/src/calendar/seal crates/email/Cargo.toml crates/email/src/message/mod.rs crates/email/src/message/za tests/Cargo.toml Cargo.lock
git commit -m "Mail sealing primitives: the mail wrapping key, raw bundle helpers, the sealed blob and the stub"
```

### Task 2: Keys on IMAP and POP3 sessions, the keyless-login refusal

**Files:**
- Modify: `crates/common/src/za/mod.rs` (the client texts and trace details as `pub const`s)
- Modify: `crates/imap/src/op/authenticate.rs:69-116` (`Session::authenticate`), `crates/imap/src/core/mod.rs:196-204` (`refresh_access_token`)
- Modify: `crates/pop3/src/op/authenticate.rs:63-113` (`handle_auth`)
- Modify: `tests/src/za/caches.rs:34-60` (the `keys_only_over_http` claim and assertions)
- Create: `tests/src/za/mail/mod.rs`, `tests/src/za/mail/login.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod mail;` and the call `mail::login::test(&mut test).await;` after `index_task::test_sealed_archive`)
- Test: `tests/src/za/mail/login.rs`

**Interfaces:**
- Consumes: `Server::authenticate_with_keys(&AuthRequest) -> trc::Result<(AccessToken, Option<Arc<SessionKeys>>)>` (`crates/common/src/auth/authentication.rs:57`), `AccessToken::{with_session_keys, session_keys, credential_id}`, `AccessToken::renew`, `Server::za_is_key_account`, `vault::session::live::session_keys` (test mode), `ImapConnection`, `Pop3Connection`, the raw-input `Conn` for OAUTHBEARER/XOAUTH2 lines, `za_app_password`, `za_post("totp", ..)` as `totp::test` enrols it.
- Produces, in `common::za`: `pub const ZA_MAIL_LOGIN_REFUSED: &str`, `ZA_MAIL_KEYLESS`, `ZA_MAIL_SEARCH_REFUSED`, `ZA_MAIL_UNREADABLE` (texts of Global Constraints) and `ZA_TRACE_MAIL_LOGIN: &str = "zero-access: mail login without session keys"`, `ZA_TRACE_MAIL_KEYLESS: &str = "zero-access: mail access without session keys"`.
- Produces, in IMAP: `Session::authenticate` calls `authenticate_with_keys`; after the `ImapAuthenticate` permission check and before the concurrency limiter, `None` keys with `self.server.za_is_key_account(access_token.account_id()).await?` true gives `trc::event!(Security(SecurityEvent::Unauthorized), SpanId = self.session_id, AccountId = account_id, Details = ZA_TRACE_MAIL_LOGIN)` and `Err(trc::ImapEvent::Error.into_err().details(ZA_MAIL_LOGIN_REFUSED).code(ResponseCode::NoPerm).id(tag))` (the state stays `NotAuthenticated`, `auth_failures` unchanged); `Some(keys)` gives `access_token.with_session_keys(keys)` before `SessionData::new`. `SessionData::refresh_access_token` ends with `.map(|token| match self.access_token.session_keys() { Some(keys) => token.with_session_keys(keys.clone()), None => token })`. STARTTLS needs no change: `Session::into_tls` moves the state through `try_replace_stream_tx` and `SessionData::replace_stream_tx` (`core/mod.rs:172-221`), which move `access_token` with its keys; the test pins it. `pub fn za_key_session(&self) -> bool` on `SessionData` (`self.access_token.session_keys().is_some()`) and on `Session` (false when `NotAuthenticated`, else the data's), used by Task 9.
- Produces, in POP3: `handle_auth` the same way; the refusal is `Err(trc::Pop3Event::Error.into_err().details(ZA_MAIL_LOGIN_REFUSED))` (written as `-ERR <text>`, no disconnect), and `State::Authenticated` holds the token with keys; `pub fn za_key_session(&self) -> bool` on `State`.
- Produces, in tests: `tests/src/za/mail/mod.rs` with `pub mod login;` (later tasks add their modules) and shared helpers `pub async fn key_mail_account(test: &mut TestServer, name: &'static str, password: &'static str) -> Account` (`create_key_user_account(name, password, "Key mail", &[], user_permissions())` plus `insert_account`), `pub async fn deliver(test: &TestServer, from: &str, to: &str, message: &str)` (`SmtpConnection::connect().ingest(..)` then `wait_for_delivery`), `pub fn message_with(subject: &str, body: &str) -> String` (a minimal RFC 5322 message with a `Message-ID`), and `pub fn live_keys(account_id: u32) -> usize` (`vault::session::live::session_keys`).

- [ ] **Step 1: Write the failing test `tests/src/za/mail/login.rs`**

`pub async fn test(test: &mut TestServer)` creates `keymail1@example.com` with `STRONG`, an app password, and runs, in order:
1. `password_login_holds_keys`: `ImapConnection::connect(b"_m ")`, `authenticate(NAME, STRONG)`; `live_keys(id) == 1` while connected; `SELECT INBOX` succeeds; `LOGOUT`; after a 200 ms sleep `live_keys(id) == 0`.
2. `app_password_login_holds_keys`: the same with the app password; `UNAUTHENTICATE` drops the keys (`live_keys(id) == 0` while still connected); a second `AUTHENTICATE` on the same connection brings them back (`== 1`).
3. `dropped_connection_drops_keys`: authenticate, then drop the `ImapConnection` without LOGOUT; within 2 s `live_keys(id) == 0`.
4. `two_sessions_are_independent`: two connections authenticated (`live_keys == 2`); LOGOUT on one (`== 1`); `FETCH 1:* FLAGS` on the other still answers OK; LOGOUT.
5. `pop3_logins`: `Pop3Connection::connect().authenticate(NAME, STRONG)` then `STAT` answers `+OK`; `live_keys(id) == 1`; `QUIT` drops to 0; the app password the same.
6. `keyless_mechanisms_are_refused`: over the TLS `Conn` on 9992, `AUTHENTICATE OAUTHBEARER` and `AUTHENTICATE XOAUTH2` with a Bearer token obtained as `tracing::test` obtains one (its OAuth helper in `tests/src/za/tracing.rs`, made `pub(crate)` and moved to `tests/src/utils/za.rs` as `pub async fn bearer_token(name: &str, password: &str) -> String`), each answered `NO [NOPERM]` with `ZA_MAIL_LOGIN_REFUSED`, the connection still open (a following `NOOP` answers OK); `AUTHENTICATE PLAIN` as `keymail1@example.com%admin@example.com` (the master-user form, the admin's password) answered `NO [NOPERM]`; POP3 `AUTH XOAUTH2` answered `-ERR` with the text; after each refusal `live_keys(id) == 0`. A plain account's OAUTHBEARER login on the same connection type succeeds (the control).
7. `totp_account_needs_an_app_password`: enrol TOTP on the account (`za_post("totp", ..)` as `totp::test` does, with a code from `totp_url`); `AUTHENTICATE PLAIN` with the password answers `NO [AUTHENTICATIONFAILED]` ("Missing MFA token", upstream's text); with the app password it succeeds and holds keys; POP3 the same two outcomes.
8. `starttls_keeps_keys`: on plain 9991, `AUTHENTICATE PLAIN`, then `STARTTLS` and the TLS handshake (the `Conn` helper's connector over the upgraded stream), then `SELECT INBOX` and `FETCH 1:* FLAGS` on the TLS session succeed and `live_keys(id) == 1`; LOGOUT drops to 0.
9. `rf_4_password_change_mid_session`: deliver one message; session A authenticated and `SELECT INBOX`; `POST /api/vault/password` (`za_post_as` with `{"password": STRONG, "new_password": NEW}`) answers 200; session A's `FETCH 1 (BODY[])` still returns the message body; a new session with `STRONG` is refused `NO [AUTHENTICATIONFAILED]`; with `NEW` it succeeds and `FETCH 1 (BODY[])` returns the same bytes; change the password back; LOGOUT both.
Each sub-step prints its name. Cleanup: revoke the app password, disable TOTP (`za_post("totp", ..)` with `enable: false` as `totp::test` ends), delete the message, `wait_for_delivery`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`
Expected: FAIL at `password_login_holds_keys` (`live_keys(id) == 0`: the token is keyless).

- [ ] **Step 3: Implement the constants, the IMAP and POP3 changes**

The `common::za` constants; `Session::authenticate` per the Interfaces block; `refresh_access_token`; `handle_auth`; the two `za_key_session` helpers (unused until Task 9: mark `#[allow(dead_code)]` only if the warning filter flags them, with a comment naming Task 9). Update `tests/src/za/caches.rs`'s `keys_only_over_http`: its comment now says IMAP and POP3 sessions hold keys for the connection (mail spec 3.3), the key-cache assertion stays (`!caches.za_keys.contains_account(id)`: the cache is not used by mail logins), and the `server.authenticate(..)` assertion stays (keyless by construction, as SMTP and ManageSieve use it).

- [ ] **Step 4: Run the tests to verify they pass**

Run: the za suite; then the compile check, `cargo fmt` for `common`, `imap`, `pop3`, `tests`.
Expected: `login::test` passes; `caches::test`, `raw_input::test` and `totp::test` unchanged; `clean`.

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/za/mod.rs crates/imap/src/op/authenticate.rs crates/imap/src/core/mod.rs crates/pop3/src/op/authenticate.rs crates/pop3/src/lib.rs tests/src/za
git commit -m "Mail sessions hold the session keys; keyless logins to a key account are refused"
```

### Task 3: Sealed ingest: delivery under `pk`, session writes under `mk`

**Files:**
- Modify: `crates/email/src/message/index/metadata.rs:100-250` (split `index_message` into `build_message_metadata` and `index_metadata`), `crates/email/src/message/index/mod.rs` (the `IndexMessage` trait gains `index_metadata`)
- Modify: `crates/email/src/message/ingest.rs` (the key-account branch: after the account read at `:150`; the thread lookup at `:200-203`; the duplicate drop at `:208`; the iMIP gate at `:367` unchanged; the encryption gate at `:500-508`; the staging and indexing at `:565-658`; the `MergeThreads` condition at `:665`; the event at `:697-717`; `add_account_spam_sample` at `:876`)
- Create: `tests/src/za/mail/delivery.rs`, `tests/src/za/mail/writes.rs`
- Modify: `tests/src/za/mail/mod.rs`, `tests/src/za/mod.rs` (the two calls after `mail::login::test`)
- Test: the two new modules

**Interfaces:**
- Consumes: Task 1's `za::{new_dek, seal_mail_envelope, MailWrap, seal_mail_blob, seal_stub}`; `Server::za_served_public_key(account_id) -> trc::Result<Option<[u8; 32]>>` (`crates/common/src/auth/vault.rs:552`, `Some` only for an `Active` record); `AccessToken::za_keys_for`; `AccountCache::is_key_account`; `put_temporary_blob(account_id, data, 60)`.
- Produces, in `email::message::index`:
  - `pub fn build_message_metadata<'x>(message: mail_parser::Message<'x>, extra_headers: Vec<u8>, extra_headers_parsed: Vec<mail_parser::Header<'x>>, blob_hash: BlobHash, received_at: u64) -> MessageMetadata` (lines 111-227 of today's `index_message`, unchanged in effect).
  - On the `IndexMessage` trait: `fn index_metadata(&mut self, tenant_id: Option<u32>, metadata: MessageMetadata, data: MessageData) -> trc::Result<&mut Self>` (the `BlobOp::Link`, the `ObjectIndexBuilder` with `data`, the `EmailField::Metadata` set: today's lines 229-247). `index_message` keeps its signature and body-equals `self.index_metadata(tenant_id, build_message_metadata(message, extra_headers, extra_headers_parsed, blob_hash, received_at), data)`.
- Produces, in `email_ingest`, the key-account branch, keyed on `let za_key = account.is_key_account();` right after the account read:
  - `IngestSource::Restore` with `za_key`: `trc::event!(Security(Unauthorized), AccountId, Details = "zero-access: restore of a key account's message")` and `Err(MessageIngest(Error).ctx(Code, 550).ctx(Reason, "zero-access: restore not supported for key accounts"))`.
  - The wrap is chosen once, before any write: `MailWrap::Session(keys)` when `params.access_token.za_keys_for(account_id)` is `Some`; otherwise, for `IngestSource::Smtp`, `MailWrap::Public(&served)` with `served = self.za_served_public_key(account_id).await?`, and when that is `None`: `trc::error!(StoreEvent::UnexpectedError ... AccountId = account_id, Reason = "zero-access: delivery without a served public key")` returned as the error (a `TemporaryFailure` in `deliver_message`); for `Jmap`/`Imap` without keys: `Err(SecurityEvent::Unauthorized.into_err().details(ZA_TRACE_MAIL_KEYLESS))` (the dispatcher and the login gate make this unreachable).
  - With `za_key`: `message_id = Some("[redacted]".into())` after the header walk; `find_thread_id` is not called and `thread_result` is `ThreadResult { thread_id: None, thread_hash: CheekyHash::default(), merge_ids: vec![], duplicate_ids: vec![] }` (so the existing "no thread id" arm logs the container and sets `thread_id = document_id`); the duplicate drop is skipped (its condition gains `&& !za_key`); `train_spam = None`; `do_encrypt = false`; at the staging point the branch is: `let dek = new_dek(); let envelope = seal_mail_envelope(&dek, wrap, account_id)?; let sealed = seal_mail_blob(&dek, &envelope, account_id, raw_message.as_ref()); let (blob_hash, blob_hold) = self.put_temporary_blob(account_id, &sealed, 60).await.map(|(h, op)| (h, Some(op)))?;` (never `params.blob_hash`); the index step becomes `batch.index_metadata(tenant_id, seal_stub(build_message_metadata(message, extra_headers.into_bytes(), extra_headers_parsed, blob_hash.clone(), received_at), &dek, &envelope, account_id).map_err(|e| seal_error(e, account_id, document_id))?, data)?` with no `Threading` set; the `IndexDocument` task is scheduled as today; the `MergeThreads` condition gains `&& !za_key`; `MessageData.size` stays `(message.raw_message.len() + extra_headers.len()) as u32` computed before `message` is consumed.
  - Without `za_key`: unchanged, except the fence of Task 4.
  - `add_account_spam_sample` starts with `if self.za_is_key_account(account_id).await? { return Ok(()); }` (spec 6.4: every caller, IMAP STORE, COPY/MOVE, Email/set, Email/copy, is covered here).
- Produces, in tests: `tests/src/za/mail/mod.rs` gains `pub mod delivery; pub mod writes;` and the helpers `pub async fn stored_metadata(test: &TestServer, account_id: u32, document_id: u32) -> Archive<AlignedBytes>` (field 71), `pub async fn stored_blob(test: &TestServer, hash: &BlobHash) -> Vec<u8>`, `pub async fn message_ids(test: &TestServer, account_id: u32) -> Vec<u32>` (the cache's document ids, sorted), `pub async fn assert_sealed(test: &TestServer, account_id: u32, document_id: u32, wrap: u8)` (the archive is a stub by `is_stub`; `raw_headers[2] == wrap`; the blob at the stub's hash starts with `MAIL_FORMAT_V1` and its wrap byte is `wrap`; no `Threading` key for the document: iterate `IndexPropertyClass::Hash` keys of property 90 for the account), `pub async fn open_message(test: &TestServer, account_id: u32, password: &str, document_id: u32) -> (MessageMetadata, Vec<u8>)` (`za_unlock` then Task 1's `open_stub` with `za_private_key` and `open_mail_blob`), and `pub async fn spam_samples(test: &TestServer) -> usize` (count of `RegistryClass::Item` sample records, through `admin.registry_get_all::<SpamTrainingSample>()`).

- [ ] **Step 1: Write the failing tests in `tests/src/za/mail/delivery.rs`**

`pub async fn test(test: &mut TestServer)` with `keymail2@example.com` (key) and `plainmail1@example.com` (plain), in order:
1. `pending_account_gets_a_temporary_failure`: `create_passwordless_user_account("keymail3@example.com", ..)`, `za_setup_token` only (state `PendingSetup`); `deliver(.., "sender@example.com", "keymail3@example.com", message_with("pending", "body"))` without `wait_for_delivery`; after `wait_for_tasks`, `queued_recipients(test)` still contains the address (the queue holds the retry) and `message_ids(test, id3).is_empty()`; the spool's queued message has a temporary status (`test.all_queued_messages()`: one message, its recipient not `Completed`); then `za_setup(..)`; the module's setup registers an `MtaDeliverySchedule` whose `retry` is one-second intervals and selects it for the local route through the `MtaOutboundStrategy` schedule expression (the registry objects `jmap_tests` configures at `tests/src/jmap/mod.rs:137-158` and `crates/registry/src/schema/structs.rs:3603-3620`), so the queue retries within a second; `wait_for_delivery` then completes and `message_ids(test, id3).len() == 1`, `assert_sealed(.., WRAP_PK)`.
2. `delivery_is_sealed_under_pk`: deliver `message_with("hello subject-canary", "body-canary")` to keymail2; `assert_sealed(.., WRAP_PK)`; `let (meta, raw) = open_message(..)`; `raw` equals the delivered bytes after the server's prepended headers are accounted for (the blob is the original message as received; `meta.raw_headers` starts with `Delivered-To:`); `meta.preview` contains `body-canary`; `MessageData.size` (from the cache) equals `meta.raw_headers.len() + raw.len() - meta.blob_body_offset`; `thread_id == document_id`.
3. `same_message_to_a_key_and_a_plain_account`: one `ingest("sender@example.com", &["keymail2@example.com", "plainmail1@example.com"], ..)`; the plain account's blob hash differs from the key account's; the plain account's archive is not a stub; `spam_samples(test)` unchanged by the delivery (the plain account's sample, if any, is counted before and after with the key account's share asserted zero: `SpamClassifier` is configured in this module's setup with `min_ham_samples: 10, min_spam_samples: 10` as `imap_tests` does, so samples are written for plain accounts; the count rises by exactly the plain account's one).
4. `sieve_fileinto_vacation_redirect`: install a Sieve script for keymail2 over ManageSieve or JMAP `SieveScript/set` (`fileinto "Archive"` when the subject contains `file-me`, `redirect "plainmail1@example.com"` when it contains `redirect-me`, and `vacation "away text"`); deliver three messages; the `file-me` one lands in a new `Archive` mailbox sealed under `pk`; the `redirect-me` one reaches plainmail1 as plaintext (its copy in keymail2's INBOX sealed); the vacation reply to the sender is queued (`queued_recipients` contains `sender@example.com` before `wait_for_delivery`); remove the script.
5. `imap_and_jmap_flags_and_moves_touch_no_content`: `STORE +FLAGS (\Seen $Junk)` on a sealed message, `MOVE` to `Junk Mail`, Email/set `keywords` update, `COPY` back to INBOX within the account: the archive and blob of the copy are byte-identical to the original's (`stored_metadata` bytes equal, same blob hash); `spam_samples(test)` unchanged after the `$Junk` flag and the Junk move; `EXPUNGE` and Email/set destroy work; after `wait_for_tasks` the metadata and the blob link are gone.
6. `restore_is_refused`: write a `Task::RestoreArchivedItem` straight to the store for keymail2 with a planted plaintext blob (as `index_task::test` writes tasks); after `wait_for_tasks_skip_failures`, `message_ids` unchanged and the task gone (a permanent failure).
Cleanup as the module rule says.

- [ ] **Step 2: Write the failing tests in `tests/src/za/mail/writes.rs`**

`pub async fn test(test: &mut TestServer)` with `keymail2@example.com` (re-created if `delivery` destroyed it; each module owns its accounts, so use `keymail4@example.com`) and `plainmail2@example.com`:
1. `append_is_sealed_under_mk`: `imap.append("INBOX", &message_with("appended", "a"))` returns an APPENDUID; `assert_sealed(.., WRAP_MK)`; `open_message` returns the appended bytes.
2. `email_set_create_is_sealed_under_mk`: `client.email_create(..)` with a text body and a keyword; `assert_sealed(.., WRAP_MK)`.
3. `email_import_reseals_the_upload`: upload a message (Task 6 seals uploads; until then, this step imports through `client.email_import` and asserts only that the message blob's hash differs from the upload's hash and the message is sealed under `mk`; Task 6 extends it).
4. `no_sample_on_session_writes`: `spam_samples(test)` unchanged after APPEND into `Junk Mail`, Email/set with `$Junk`, and Email/set create into Junk.
5. `rf_3_append_into_plain_share`: plainmail2 grants `Shared Folders` access to keymail4 (IMAP `SETACL INBOX keymail4@example.com lrswi` from plainmail2's session); keymail4's session `APPEND "Shared Folders/plainmail2@example.com/INBOX"` with `message_with("shared", "plain body")`; plainmail2's own session `FETCH 1 (BODY[TEXT])` shows `plain body`; plainmail2's archive is not a stub; plainmail2 removes the grant.
Cleanup as the module rule says.

- [ ] **Step 3: Run the tests to verify they fail**

Run: the za suite.
Expected: FAIL at `delivery_is_sealed_under_pk` (the archive is plaintext).

- [ ] **Step 4: Implement the metadata split and the ingest branch**

`build_message_metadata`, `index_metadata`, the branch of the Interfaces block, the `add_account_spam_sample` gate. The plain path's batch content must be unchanged: diff the batch ops of a plain ingest before and after (the existing `jmap_tests` golden files of `mail::get` are the check in Task 10).

- [ ] **Step 5: Run the tests to verify they pass**

Run: the za suite; `cargo test -p email`; the compile check; `cargo fmt` for `email`, `tests`.
Expected: pass, `clean`.

- [ ] **Step 6: Kill check**

Replace `MailWrap::Public(&served)` with `MailWrap::Session(..)` guarded away, i.e. temporarily return the plain path for SMTP: `delivery_is_sealed_under_pk` must fail. Revert.

- [ ] **Step 7: Commit**

```bash
git add crates/email/src/message/index crates/email/src/message/ingest.rs tests/src/za
git commit -m "Sealed ingest: delivery under the public key, session writes under the mail key"
```

### Task 4: The provisioning fence, the recheck, the store restriction

**Files:**
- Modify: `crates/email/src/message/ingest.rs` (the assertion after `batch.with_account_id(account_id)` at `:593`), `crates/email/src/message/copy.rs` (the assertion after `batch.with_account_id(to_account_id)` at `:186`; the key-account refusal after the `to_account` read at `:96`)
- Modify: `crates/common/src/auth/vault.rs` (`za_vault_delete` after `za_vault_commit`, line 431; `vault_delete_store` next to `vault_write_store`)
- Modify: `crates/http/src/api/vault.rs` (`za_assert_no_calendar_data` at `:775-791` becomes `za_assert_no_data`; `za_setup_token` at `:636-752`: the store refusal before the state check, the recheck between `za_commit` at `:717-724` and the marker write at `:730`; `za_setup` at `:885-887` calls the renamed check)
- Modify: `crates/common/src/za/test_hooks.rs` (`INGEST_PAUSE`, `COPY_PAUSE`, `SETUP_TOKEN_PAUSE` account-keyed pause slots with `set_*` and `*_pause_point`; `FORCE_SQL_STORE: AtomicBool` with `set_force_sql_store(bool)` and `force_sql_store() -> bool`), `crates/email/Cargo.toml` (`test_mode = ["common/test_mode"]`, as `dav`'s is), `crates/http/Cargo.toml` (check its `test_mode` forwards to `common/test_mode`; add if not)
- Create: `tests/src/za/mail/fence.rs`
- Modify: `tests/src/za/mail/mod.rs`, `tests/src/za/mod.rs`
- Test: `tests/src/za/mail/fence.rs`, unit tests in `vault.rs`

**Interfaces:**
- Consumes: `AssertValue::None`, `PrincipalField::ZeroAccessVault`, `BatchBuilder::{with_collection, with_document, assert_value}`, `VaultRead { record, cas }`, `vault_batch_write`'s pattern, `Store::is_pg_or_mysql()`, `za_has_documents(account_id, Collection::Email)`, `HookParked`, `common::za::test_hooks::Pause`.
- Produces, in `email_ingest` and `copy_message`: when the destination account is not a key account, immediately after `batch.with_account_id(..)`: `batch.with_collection(Collection::Principal).with_document(0).assert_value(PrincipalField::ZeroAccessVault, AssertValue::None);` (the following ops re-select their collection and document as they do today; `batch.is_atomic()` turns false, which is what the vault batch does). `copy_message` refuses a key account on either side before any read: `Err(SecurityEvent::Unauthorized.into_err().details("zero-access: mail copy across accounts"))` (Task 7's callers answer the client first; this is the fence against a classification that changed in between). Test-mode pause points: `common::za::test_hooks::ingest_pause_point(account_id).await` right after the account read in `email_ingest`, `copy_pause_point(to_account_id)` after the `to_account` read in `copy_message`.
- Produces, in `common::auth::vault`: `pub async fn za_vault_delete(&self, account_id: u32, previous: &VaultRead) -> trc::Result<()>`: one batch asserting `AssertValue::Hash(previous.cas)` on field 150 and clearing it, then `za_invalidate_account`; a lost assertion is `Err(e)` with `e.is_assertion_failure()`. Doc: "The recheck's abort (mail spec 6.1): removes a pending record this process wrote, conditional on the bytes it wrote."
- Produces, in `http::api::vault`: `async fn za_assert_no_data(server, account_id) -> trc::Result<Option<HttpResponse>>`: the calendar checks as today, then `za_has_documents(account_id, Collection::Email)` giving `conflict("account already holds mail")`. `za_setup_token`: before the state check, `if server.store().is_pg_or_mysql() || force_sql_store() { return Ok(conflict("key accounts are not supported on PostgreSQL or MySQL")); }` (the hook call under `#[cfg(feature = "test_mode")]`); after `za_commit` and before the marker write: `setup_token_pause_point(account_id).await` (test mode), then `if let Some(response) = za_assert_no_data(server, account_id).await? { if !has_marker { let written = server.za_vault_record(account_id).await?.filter(|r| r.record.revision == record.revision); if let Some(written) = written { server.za_vault_delete(account_id, &written).await?; } } return Ok(response); }` (a lost delete assertion is reported as the same 409: the record is then someone else's). `za_setup` calls `za_assert_no_data`.
- Produces, in tests: `tests/src/za/mail/fence.rs`.

- [ ] **Step 1: Write the failing unit test for `za_vault_delete` in `crates/common/src/auth/vault.rs`'s tests**

```rust
#[tokio::test]
async fn a_pending_record_is_deleted_only_at_its_own_revision() {
    let server = test_server().await; // the module's existing helper
    let mut record = VaultRecord::pending(vec![1; 32], 0);
    server.za_vault_commit(9, &mut record, None, None).await.unwrap();
    let read = server.za_vault_record(9).await.unwrap().unwrap();
    let mut rotated = read.record.clone();
    server.za_vault_commit(9, &mut rotated, Some(&read), None).await.unwrap();
    assert!(server.za_vault_delete(9, &read).await.unwrap_err().is_assertion_failure(), "stale cas");
    let read = server.za_vault_record(9).await.unwrap().unwrap();
    server.za_vault_delete(9, &read).await.unwrap();
    assert!(server.za_vault_record(9).await.unwrap().is_none());
}
```

- [ ] **Step 2: Write the failing tests in `tests/src/za/mail/fence.rs`**

`pub async fn test(test: &mut TestServer)`, each case on a fresh passwordless account it creates (`fence<n>@example.com`), in order:
1. `forward_race_delivery_loses`: `create_passwordless_user_account`; `HookParked::start(id, test_hooks::set_ingest_pause, deliver_future)` where the future runs `SmtpConnection::ingest` of `message_with("race", "race-body")` (the LMTP reply arrives only after the pause releases, so the future returns the reply); while parked: `za_setup_token` and `za_setup` complete (200); `finish()`; `wait_for_delivery` is not usable while the retry is pending: assert `message_ids(id).is_empty()` right after finish, then force the retry as `delivery::test` step 1 does; the retry ends with one message, `assert_sealed(.., WRAP_PK)`; the leak scan of the account (`leak::scan`) reports no plaintext archive.
2. `forward_race_with_a_staged_blob`: the same with a Sieve script that modifies the message (`addheader "X-Sieve-Fence" "1"`), so the ingest stages its blob before the batch; while parked, plainmail (a plain account in this module) ingests the identical bytes through its own delivery; after `finish()`: the staged hash is linked by the plain account and readable there; after the retry, the key account's message is sealed and its hash differs; `test.blob_expire_all()` plus a purge (`PurgeStore`/`PurgeBlobs` maintenance task scheduled directly, as `blob.rs` tests do) leaves the plain account's blob readable and no temporary link of the key account.
3. `reverse_race_setup_token_aborts`: `create_passwordless_user_account`; `HookParked::start(id, test_hooks::set_setup_token_pause, za_setup_token_future)`; while parked, deliver one message (`wait_for_delivery` completes: the record is not yet written); `finish()` returns a 409 `account already holds mail`; `za_vault_record(id)` is `None` and the account has no marker (`za_registry_account`'s credentials are empty); delete the message (`destroy_all_mailboxes_for_account`) and `wait_for_tasks`; a fresh `za_setup_token` succeeds and `za_setup` completes.
4. `reverse_race_reissue_keeps_the_marker`: an account with a marker and a record (a pending account whose first token expired is simulated by `za_setup_token` twice); the second call paused the same way with a message delivered in between is impossible (the fence refuses the delivery: assert that path instead: the delivery gets a temporary failure while the record exists, and the reissue answers 200).
5. `copy_message_race`: a plain account `fence5` with one plain message and a plain account `fence6`; `HookParked::start(id6, test_hooks::set_copy_pause, jmap_email_copy_future)` (admin's client copying from `fence5` to `fence6`); while parked, `fence6` is provisioned (`za_setup_token`, `za_setup`); `finish()` returns a JMAP `forbidden` (Task 7 lands the client refusal; until then the copy's lost assertion surfaces as a `serverFail`; assert `message_ids(id6).is_empty()` and no plaintext archive, and tighten the status assertion in Task 7).
6. `stress_the_fence`: 64 iterations in a loop: an account `fence7` reset to no record (`za_vault_delete` through a test helper, cache invalidated), then two tasks spawned at once: (a) `email_ingest` called at server level with `IngestSource::Smtp` and a keyless token (as `thread_merge` calls it) for `fence7`, (b) `za_vault_commit(id, &mut VaultRecord::pending(..), None, None)`; after both settle, if the record exists and the ingest returned `Ok`, the ingest's batch must have committed before the record: assert `message_ids` is empty whenever the ingest returned an assertion failure, and whenever it returned `Ok`, delete the record and assert the message is plaintext (the record came second); never both a record and a plaintext message written after it. Run on RocksDB in CI; on FoundationDB and SQLite when `STORE` selects them.
7. `pg_and_mysql_are_refused`: `test_hooks::set_force_sql_store(true)`; `za_setup_token` on a fresh account answers 409 with `key accounts are not supported on PostgreSQL or MySQL`; `set_force_sql_store(false)`; the same call answers 200.
8. `setup_refuses_existing_mail`: a plain account with one delivered message: `za_setup_token` answers 409 `account already holds mail`; delete the message; 200.
Cleanup: destroy the fence accounts (`destroy_account`) and their messages.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p common auth::vault`, then the za suite.
Expected: compile error on `za_vault_delete`; `forward_race_delivery_loses` ends with a plaintext message.

- [ ] **Step 4: Implement the assertions, the helper, the checks and the hooks**

Per the Interfaces block. `crates/email`'s `test_mode` feature must forward to `common/test_mode` for the pause points to compile in the `tests` build.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p common auth::vault`; the za suite; the compile check; `cargo fmt` for `common`, `email`, `http` (manifest form), `tests`.
Expected: pass, `clean`.

- [ ] **Step 6: Kill checks**

(a) Remove the ingest assertion: `forward_race_delivery_loses` fails (a plaintext message in a key account). (b) Remove the recheck: `reverse_race_setup_token_aborts` fails (200 and a stranded account). Revert each.

- [ ] **Step 7: Commit**

```bash
git add crates/email/src/message crates/email/Cargo.toml crates/common/src/auth/vault.rs crates/common/src/za/test_hooks.rs crates/http/src/api/vault.rs crates/http/Cargo.toml tests/src/za
git commit -m "The provisioning fence: ordinary writers assert no vault record, setup-token rechecks and refuses SQL stores"
```

### Task 5: The unsealed view at every reader, the visible-field document, state-only push

**Files:**
- Create: `crates/email/src/message/za/view.rs` (`pub mod view;` in `za/mod.rs`)
- Modify: `crates/imap/src/op/fetch.rs:360-410` (the metadata read and the blob read), `crates/pop3/src/op/fetch.rs:30-64`, `crates/jmap/src/email/get.rs:160-200`, `crates/jmap/src/email/snippet.rs:140-180`, `crates/jmap/src/submission/set.rs:66,359,553-569,651-655` (the token parameter and the two reads), `crates/jmap/src/api/request.rs:500-507` (passes `access_token` to `email_submission_set`), `crates/jmap/src/blob/download.rs:39-97`
- Modify: `crates/services/src/task_manager/index.rs:441-486` (`build_email_document`), `crates/services/src/state_manager/push.rs:369-380` (the conversion condition), `crates/services/src/state_manager/email_push.rs:60-70` (the defence-in-depth return)
- Create: `tests/src/za/mail/reads.rs`; Modify: `tests/src/za/mail/mod.rs`, `tests/src/za/mod.rs`
- Test: unit tests in `view.rs` (the offsets corpus), `tests/src/za/mail/reads.rs`

**Interfaces:**
- Consumes: Task 1's `za::{is_stub, open_stub, open_mail_blob, stub_needs_private_key, blob_needs_private_key}`; `Server::za_private_key(&SessionKeys) -> trc::Result<&Secret>` (`vault.rs:990`, runs the self-check once per `SessionKeys`); `groupware::calendar::seal::seal_error(SealError, account_id, document_id) -> trc::Error` (`DataCorruption`); `Archiver`, `<Archive<AlignedBytes> as Deserialize>::deserialize`; `AccessToken::za_keys_for`; `Server::za_is_key_account`; `IndexDocument::{new, with_account_id, with_document_id, index_unsigned}`; `EmailSearchField::{ReceivedAt, Size}`; `MessageData` at `ValueKey::archive`.
- Produces, in `email::message::za::view`, on `impl Server`:
  - `pub async fn za_mail_keys(&self, access_token: &AccessToken, account_id: u32) -> trc::Result<Option<Arc<SessionKeys>>>`: `Some(keys)` when `access_token.za_keys_for(account_id)` holds them; `None` when the account is not a key account; otherwise `trc::event!(Security(Unauthorized), AccountId, Details = ZA_TRACE_MAIL_KEYLESS)` and `Err(SecurityEvent::Unauthorized.into_err().details(ZA_MAIL_KEYLESS))`. The IMAP callers add `.code(ResponseCode::NoPerm)` and POP3 writes `-ERR`; a non-JMAP event serializes as `serverUnavailable` on JMAP (`crates/jmap-proto/src/error/method.rs:167-173`), so the JMAP readers wrap their calls with `pub fn za_jmap_forbidden(err: trc::Error) -> trc::Error` (also in `view.rs`): a `Security(Unauthorized)` error becomes `JmapEvent::Forbidden.into_err().details(ZA_MAIL_KEYLESS)`, every other error passes through.
  - `pub async fn za_metadata_view<'x>(&self, stored: &'x Archive<AlignedBytes>, keys: Option<&SessionKeys>, account_id: u32, document_id: u32) -> trc::Result<Cow<'x, Archive<AlignedBytes>>>`: `stored.unarchive::<MessageMetadata>()`; when `!is_stub(..)` (a plain account, or a legacy plaintext archive, spec 6.6) `Cow::Borrowed(stored)`; when a stub and `keys` is `None`, the keyless error of `za_mail_keys`; when a stub with keys: `private_key = if stub_needs_private_key(..)? { Some(self.za_private_key(keys).await?) } else { None }`, `open_stub(..)` mapped through `seal_error(e, account_id, document_id)` with `Reason = "zero-access: unreadable message"`, re-serialized with `Archiver::new(metadata).serialize()`, deserialized into an `Archive<AlignedBytes>` whose `version` is copied from `stored`, `Cow::Owned`. The doc comment repeats `unseal_event_archive`'s rule: the view is never written back, never an assertion's current value.
  - `pub async fn za_open_message_blob(&self, stored_is_stub: bool, keys: Option<&SessionKeys>, account_id: u32, document_id: u32, bytes: Vec<u8>) -> trc::Result<Vec<u8>>`: `bytes` unchanged when `!stored_is_stub`; otherwise the keyless error without keys, or `open_mail_blob` with the lazily fetched private key, errors through `seal_error`; the opened plaintext is moved into the returned `Vec` (the reader holds it for the request, as today).
  - `pub async fn za_open_upload_blob(&self, keys: &SessionKeys, account_id: u32, bytes: Vec<u8>) -> trc::Result<Vec<u8>>`: the same for a `Reserved` blob (always sealed for a key account; `blob_needs_private_key` decides the key).
- Produces, per reader (each site: read the stored archive as today, obtain `keys` once per operation with `za_mail_keys`, replace the archive with the view, and open the blob when read):
  - IMAP FETCH (`fetch.rs`): `keys` once before the message loop from `self.access_token` and `mailbox.id.account_id`; per message the view; `needs_blobs` opens the blob with `za_open_message_blob(is_stub(stored), ..)`. A `DataCorruption` error for one message: the loop skips it (`continue`) after `trc::error!` and sets a flag; at the end the tagged response is `NO [SERVERBUG] ZA_MAIL_UNREADABLE` when the flag is set and every other message was served. The keyless error (`Security`) is returned as `ImapEvent::Error ... .code(ResponseCode::NoPerm)` with `ZA_MAIL_KEYLESS` (it is not a `Security` error on the wire). SELECT and IDLE reach the same code and need no change.
  - POP3 RETR/TOP (`pop3/op/fetch.rs`): the view and the opened blob; a `DataCorruption` is `-ERR ZA_MAIL_UNREADABLE` for that message; keyless is `-ERR ZA_MAIL_KEYLESS`.
  - Email/get (`get.rs:160-200`): the view; the blob opened when `needs_body`; a `DataCorruption` puts the id in `not_found` and continues; the blob id built for sections keeps the stub's hash (the sealed blob) and the translated offsets, as today, because the view's `blob_hash` is that hash.
  - SearchSnippet/get (`snippet.rs`): the view and the opened blob (Task 8 refuses term searches before this runs; with no terms the loop returns bare snippets and reads nothing).
  - EmailSubmission/set (`submission/set.rs`): `email_submission_set` and `send_message` gain `access_token: &AccessToken`; the dispatcher passes it; the archive read becomes the view and the blob read is opened; the Bcc strip runs on the opened bytes.
  - `blob_download` (`download.rs`): after `has_access_blob`, when the id is a sealed source (Task 6's `za_is_sealed_source`), `keys` from `za_mail_keys(access_token, account_id)` (`Some` for a sealed source, since a sealed source belongs to a key account and a keyless token already errored); a `Reserved` id: the whole blob opened with `za_open_upload_blob(&keys, ..)`, then the section slice and transfer decoding applied to the opened bytes (the `get_blob_section` decode logic lifted into a local fn `decode_section(bytes, section) -> Option<Vec<u8>>` reused by both branches; `get_blob_section` itself is unchanged for plain blobs); a `Linked` Email id: the stub read, the blob opened with `za_open_message_blob`, then the existing two branches on the view: a section id slices the opened blob, a whole-message id reconstructs `raw_headers || opened[blob_body_offset..]` when the view's `root_part().offset_body != blob_body_offset`.
  - The index task (`index.rs:441-486`): after the archive read: `if server.za_is_key_account(account_id).await? || is_stub(&metadata) { let size = MessageData at ValueKey::archive → size, NotFound when absent; return Ok(BuildResult::Document(IndexDocument::new(SearchIndex::Email).with_account_id(account_id).with_document_id(document_id) + index_unsigned(ReceivedAt, rcvd_attach & MESSAGE_RECEIVED_MASK) + index_unsigned(Size, size))) }` before the blob read.
  - Push (`push.rs:369-380`): the conversion condition becomes `!subscription.server.email_push.iter().any(..) || server.za_is_key_account(account_id).await.unwrap_or(true)`; `build_email_push_object` returns `Ok(None)` first thing for a key account.
- Produces, in tests: `tests/src/za/mail/reads.rs`; a helper `pub async fn corrupt_stub(test, account_id, document_id)` (rewrites field 71 with one flipped byte inside `raw_headers`) and `pub async fn corrupt_blob(test, hash)` (rewrites the blob with one flipped byte through `blob_store().put_blob`).

- [ ] **Step 1: Write the failing offsets corpus unit test in `view.rs`**

Inline messages in a `const CORPUS: &[&str]`: a simple text message; a multipart/alternative with text and HTML; a multipart/mixed with a base64 attachment named `report.pdf`; a nested `message/rfc822` whose inner message has its own attachment; an 8-bit body with a long folded header; a message with a header block only (no body). For each: parse with `MessageParser`, build the full metadata with `build_message_metadata(message, b"Delivered-To: x@example.com\r\n".to_vec(), parsed extra header, hash, 1_700_000_000)`, seal the stub, open it again, and assert for every part of every `contents` entry that `offset_header..offset_end` and `offset_body..offset_end` of the opened view, applied to the reconstructed message (`raw_headers || raw[blob_body_offset..]`), equal the same ranges of the full metadata applied to the same reconstruction; and that `decode_contents` of each text part equals the original text.

- [ ] **Step 2: Write the failing tests in `tests/src/za/mail/reads.rs`**

`pub async fn test(test: &mut TestServer)` with `keymail5@example.com` (key) and `plainmail3@example.com` (plain), delivering the same corpus message (a multipart/mixed with a text part, a base64 attachment and a nested message/rfc822, built in the test) to both, then in order:
1. `offsets_round_trip`: FETCH `BODY[]`, `BODY[HEADER]`, `BODY[HEADER.FIELDS (Subject)]`, `BODY[1]`, `BODY[2]`, `BODY[3.1]`, `BODYSTRUCTURE`, `ENVELOPE`, `RFC822.SIZE`, `PREVIEW` on the key account equal, byte for byte after the tag, the plain account's answers (the `Delivered-To` header included); Email/get with `bodyValues`, `textBody`, `attachments`, `headers`, `preview`, `size`, `hasAttachment` returns equal JSON (ids and blob ids excluded) for both; `/jmap/download` of the attachment's section blob id returns the decoded attachment bytes for both, and of the whole-message blob id returns the reconstructed message equal to the plain account's download.
2. `pop3_retr_and_top`: RETR of the key account's message equals the plain account's; TOP with 2 lines likewise; STAT/LIST sizes equal.
3. `corrupted_stub_and_blob`: deliver two more messages; `corrupt_stub` on the second and `corrupt_blob` on the third; `FETCH 1:* (BODY[])` answers the first message, skips the others and ends `NO [SERVERBUG]`; the captured traces (the `raw_input.rs` subscriber pattern on `Store(DataCorruption)`) carry the account and document ids and the reason, nothing else; Email/get of the three ids lists two under `notFound`; POP3 RETR of the corrupted ones is `-ERR`; the mailbox listing (`Email/query` by `receivedAt`) still lists all three.
4. `legacy_plaintext_is_served_as_plaintext`: plant a plaintext message in the key account with `plant_plain_message(test, account_id, raw)` (a new `utils/za.rs` helper: `email_ingest`'s plain path cannot be forced, so the helper writes field 71 and `MessageData` through `index_message` on a `BatchBuilder` and links the blob, as `plant_event` does for calendars); FETCH `BODY[]` returns it; Email/get returns it; the classification helper of Task 3 (`assert_sealed`) reports it is not a stub while the sealed neighbour is, both with non-empty `contents` (the `raw_headers` marker alone tells them apart).
5. `threads_are_singletons`: deliver three messages with `References`/`In-Reply-To` chaining and the same subject; `Thread/get` returns three threads of one email each; `THREAD REFERENCES UTF-8 ALL` returns three singleton groups; `Email/query` with `collapseThreads: true` returns three.
6. `submission_strips_bcc`: Email/set creates a draft with `To`, `Bcc` and a body in the key account; `EmailSubmission/set` with the identity sends it (the za suite's local queue: `queued_recipients` lists the To and the Bcc addresses; `wait_for_delivery` after routing them to `plainmail3@example.com` and `plainmail4@example.com` by alias); the delivered copy at `plainmail3` has no `Bcc` header; `onSuccessUpdateEmail` moved the draft to Sent and the Sent copy is still sealed.
7. `push_is_state_only`: a push subscription with `email_push` properties `["subject", "from"]` for the key account, verified against the mock push server of `tests/src/jmap/core/push_subscription.rs:107-150` (port 19000), lifted into `tests/src/utils/push.rs` as `pub fn spawn_mock_push_server() -> (mpsc::Receiver<Value>, ..)` and re-exported from its old place; a delivery produces a `StateChange` payload with `Email` and `Mailbox` types and no `EmailPush` object; the same subscription for `plainmail3` produces the object with the subject (the control); `build_email_push_object(&test.server, key_id, document_id, &config, 4096)` returns `Ok(None)` directly.
8. `rf_1_idle_delivery`: a fresh IMAP session of the key account, `SELECT INBOX`, `IDLE`; deliver a message; read `* n EXISTS`; `DONE`; `FETCH n (BODY[TEXT])` returns the body.
9. `rf_5_large_header_block`: deliver a message whose header block holds twenty `X-Pad-<n>` headers of 1000 bytes each; FETCH `BODY[HEADER]` returns all of them; Email/get `headers` likewise; the stored stub's archive bytes are smaller than the bundle (compressed).
Cleanup as the module rule says; `push_is_state_only` destroys its subscriptions.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p email za::view`, the za suite.
Expected: compile error on `za_metadata_view`; `offsets_round_trip` fails (FETCH returns an empty message).

- [ ] **Step 4: Implement `view.rs` and the reader conversions**

Per the Interfaces block, one reader at a time, running `reads::test` between them; the four JMAP readers (`get.rs`, `snippet.rs`, `submission/set.rs`, `download.rs`) map the view's and the opener's errors through `za_jmap_forbidden`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p email`; the za suite; `cargo test -p tests jmap_tests` and `imap_tests` in plain mode (the readers' plain path is unchanged); the compile check; `cargo fmt` for `email`, `imap`, `pop3`, `jmap`, `services`, `tests`.
Expected: pass, `clean`.

- [ ] **Step 6: Kill check**

In `za_metadata_view`, return `Cow::Borrowed(stored)` for a stub: `offsets_round_trip` fails with empty answers (the safety net of spec 5.2, no panic). Revert.

- [ ] **Step 7: Commit**

```bash
git add crates/email/src/message/za crates/imap/src/op/fetch.rs crates/pop3/src/op/fetch.rs crates/jmap/src/email crates/jmap/src/submission crates/jmap/src/api/request.rs crates/jmap/src/blob/download.rs crates/services/src tests/src
git commit -m "Readers open the stub and the blob through the unsealed view; the visible-field document; state-only push"
```

### Task 6: Sealed uploads and the refusal of raw consumers

**Files:**
- Modify: `crates/email/src/message/za/view.rs` (`za_put_sealed_upload`, `za_is_sealed_source`)
- Modify: `crates/http/src/request.rs:222-270` (the `/jmap/upload` 403), `crates/jmap/src/blob/upload.rs:42-131,189,199-251` (`blob_upload`, `blob_upload_many`, the data sources), `crates/jmap/src/blob/copy.rs:30-97`, `crates/jmap/src/file/set.rs:97-123,359-385`, `crates/jmap/src/file/copy.rs:177-203`
- Create: `tests/src/za/mail/uploads.rs`; Modify: `tests/src/za/mail/mod.rs`, `tests/src/za/mod.rs`, `tests/src/za/mail/writes.rs` (step 3 extended)
- Test: `tests/src/za/mail/uploads.rs`

**Interfaces:**
- Consumes: Task 1's `za::{new_dek, seal_mail_envelope, MailWrap, seal_mail_blob}`; `put_jmap_blob(account_id, data) -> trc::Result<BlobId>`; `BlobClass::{Reserved, Linked}`; `Collection::Email`; Task 5's `za_mail_keys`, `za_open_upload_blob`, `ZA_TRACE_MAIL_KEYLESS`.
- Produces, on `impl Server` in `view.rs`:
  - `pub async fn za_put_sealed_upload(&self, account_id: u32, keys: &SessionKeys, data: &[u8]) -> trc::Result<BlobId>`: a fresh DEK, `seal_mail_envelope(.., MailWrap::Session(keys), ..)`, `seal_mail_blob`, then `put_jmap_blob(account_id, &sealed)`; the quota and size checks of the callers run on `data.len()` before the call.
  - `pub async fn za_is_sealed_source(&self, blob_id: &BlobId) -> trc::Result<bool>`: `Reserved { account_id, .. }` or `Linked { account_id, collection, .. }` with `collection == u8::from(Collection::Email)`, and `za_is_key_account(account_id)`; everything else `false`.
  - `pub async fn za_refuse_raw_source(&self, blob_id: &BlobId) -> trc::Result<()>`: `Err(SecurityEvent::Unauthorized.into_err().details(ZA_MAIL_KEYLESS))` after `trc::event!(Security(Unauthorized), AccountId = <the id's account>, Details = "zero-access: raw read of a sealed source")` when `za_is_sealed_source`.
- Produces, at the sites:
  - `/jmap/upload` (`request.rs`): after `authenticate_headers` and the account id parse, before `blob_upload`: `if self.za_is_key_account(account_id.document_id()).await? && access_token.za_keys_for(account_id.document_id()).is_none() { trace ZA_TRACE_MAIL_KEYLESS; return 403 }` (an `HttpResponse` of `StatusCode::FORBIDDEN`, untraced body as the key-account branch already is).
  - `blob_upload` and `blob_upload_many`: the write is `match access_token.za_keys_for(account_id) { Some(keys) => self.za_put_sealed_upload(account_id, keys, &data), None => self.put_jmap_blob(account_id, &data) }`; `blob_upload_many` is also behind the dispatcher gate of Task 7, and refuses at its start when the account is a key account without keys (the same error as `za_mail_keys`).
  - `Blob/upload` data sources (`upload.rs:63-131`): `self.za_refuse_raw_source(&id).await?` before `has_access_blob` (the method error for the whole call: a sealed source in a data source is a programming error of the client, and a per-item `forbidden` would read as a permission problem; this is the plan's call).
  - `Blob/copy` (`copy.rs`): at the start, `za_is_key_account(account_id)` (the destination) or `za_is_key_account(request.from_account_id)` gives `trc::event!(Security(Unauthorized), AccountId, Details = "zero-access: mail copy across accounts")` and `Err(JmapEvent::Forbidden.into_err().details(ZA_MAIL_KEYLESS))`; per blob id, `za_refuse_raw_source`.
  - `FileNode/set` create and update, `FileNode/copy`: `za_refuse_raw_source(&blob_id).await?` before `has_access_blob` at the three sites.
- Produces, in tests: `tests/src/za/mail/uploads.rs`.

- [ ] **Step 1: Write the failing tests in `tests/src/za/mail/uploads.rs`**

`pub async fn test(test: &mut TestServer)` with `keymail6@example.com` (key), `keymail7@example.com` (key) and `plainmail5@example.com`, in order:
1. `upload_with_the_credential_is_sealed`: `client.upload(None, bytes, None)` (Basic) returns a blob id; the stored blob at that hash starts with `MAIL_FORMAT_V1` and does not contain the bytes; `client.download(&blob_id)` returns the bytes; `Blob/get` returns them; `Blob/upload` with a `data:asText` source likewise sealed and readable.
2. `upload_without_keys_is_refused`: `POST /jmap/upload/<keymail6 id>` with a Bearer token of keymail6 (the OAuth flow) answers 403 and no temporary link appears for the account (`SUBSPACE_BLOB_LINK` scan as the leak test does); over the WebSocket, `Blob/upload` answers `forbidden`; plainmail5's session uploading into keymail6's account id answers 403; keymail6 uploading into keymail7's account id answers 403.
3. `sealed_upload_imported_and_attached`: upload a message, `Email/import` it: the message is sealed under `mk` and its blob hash differs from the upload's; upload an attachment and Email/set create with `blobId` for a part: the created message contains the attachment bytes (Email/get `bodyValues`/download of the section); Email/set create with a `blobId` that is a section id of the key account's own earlier message (forwarding an attachment) works and the attachment bytes match.
4. `raw_consumers_are_refused`: with `m` a whole-message blob id, `s` an attachment section id and `u` an upload id of keymail6: from keymail6's own session, `Blob/upload` with a `blobId` data source of each answers `forbidden` with `ZA_MAIL_KEYLESS`; `FileNode/set` create with `blobId` each answers `forbidden`; `FileNode/set` update of an existing plain file node setting `blobId` to each answers `forbidden`; `FileNode/copy` with each answers `forbidden`; `Blob/copy` of each into plainmail5 (plainmail5's session, `fromAccountId` keymail6) answers `forbidden`; `Blob/copy` of plainmail5's own upload into keymail6 answers `forbidden`; from plainmail5's session with keymail6's ids: the same refusals (the rule is keyed by the owning account); the controls: plainmail5's own upload passes through `Blob/upload` data sources, `FileNode/set` and `Blob/copy` into keymail7? No: into another plain account (`plainmail6@example.com`), all accepted; no trace carries the blob's content.
Cleanup: file nodes destroyed, uploads left to expire (`blob_expire_all`), messages deleted.

- [ ] **Step 2: Extend `writes.rs` step 3** to assert, with the sealed upload, that the imported message's blob hash differs from the upload's hash and opens under `mk`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: the za suite.
Expected: `upload_with_the_credential_is_sealed` fails (the stored blob is the plaintext).

- [ ] **Step 4: Implement the writer, the rule and the sites**

Per the Interfaces block.

- [ ] **Step 5: Run the tests to verify they pass**

Run: the za suite; `jmap_tests` plain; the compile check; `cargo fmt` for `email`, `jmap`, `http` (manifest form), `tests`.
Expected: pass, `clean`.

- [ ] **Step 6: Commit**

```bash
git add crates/email/src/message/za/view.rs crates/http/src/request.rs crates/jmap/src/blob crates/jmap/src/file tests/src/za
git commit -m "Uploads of key accounts are sealed; raw consumers of a sealed source are refused"
```

### Task 7: Access gates: the JMAP dispatcher, cross-account copy, sharing and listing

**Files:**
- Modify: `crates/jmap/src/api/request.rs` (`za_assert_mail_keys` next to `za_assert_calendar_allowed` at `:735-743`; the arms at `:255` Email/get, `:310` Blob/get, `:500-507` EmailSubmission/set, `:610-621` Email/copy, `:622-627` Blob/copy, `:667-672` Email/import, `:674-679` Email/parse, `:695-700` SearchSnippet/get, `:713-718` Blob/upload)
- Modify: `crates/jmap/src/email/set.rs:123` (the create loop), `crates/jmap/src/email/copy.rs:59-66`, `crates/imap/src/op/copy_move.rs:244` (before the account branch), `crates/imap/src/op/acl.rs:267-290` (after `get_acl_mailbox`), `crates/jmap/src/mailbox/set.rs:612-625` (the `has_acl_changes` block), `crates/common/src/auth/access_token.rs:77-125` (`build_access_token`)
- Create: `tests/src/za/mail/gates.rs`; Modify: `tests/src/za/mail/mod.rs`, `tests/src/za/mod.rs`, `tests/src/za/mail/fence.rs` (step 5's status tightened)
- Test: `tests/src/za/mail/gates.rs`

**Interfaces:**
- Consumes: `AccessToken::za_keys_for`, `Server::za_is_key_account`, `trc::JmapEvent::Forbidden`, `SetError::forbidden()`, `ResponseCode::NoPerm`, `AccessTo { account_id, collections: Bitmap<Collection> }`, `Bitmap::remove`, `common::za::{ZA_MAIL_KEYLESS, ZA_TRACE_MAIL_KEYLESS}`.
- Produces, in `jmap::api::request`: `pub(crate) async fn za_assert_mail_keys(server: &Server, access_token: &AccessToken, account_id: Id) -> trc::Result<()>`: `Ok(())` when `access_token.za_keys_for(account_id.document_id()).is_some()` or the account is not a key account; otherwise `trc::event!(Security(Unauthorized), AccountId = .., Details = ZA_TRACE_MAIL_KEYLESS)` and `Err(JmapEvent::Forbidden.into_err().details(ZA_MAIL_KEYLESS))`. Called, after each arm's `resolve_account_id` and access assertion, for: Email/get, Blob/get, EmailSubmission/set, Email/copy (both `req.account_id` and `req.from_account_id`), Email/import, Email/parse, SearchSnippet/get, Blob/upload. Blob/copy: `za_assert_mail_keys(.., req.account_id)` plus Task 6's cross-account refusal inside `blob_copy`. Email/set: at the top of the `'create` loop in `email_set` (`set.rs:123`), once, `if !request.create.is_empty() { za_assert_mail_keys(self, access_token, request.account_id).await?; }` placed before the loop (updates and destroys stay keyless). WebSocket requests take the same arms (`handle_jmap_request` is shared), so the gate covers them without a WebSocket-specific line.
- Produces, cross-account copy: IMAP `SessionData::copy_move`, before `if src_mailbox.id.account_id == dest_mailbox.account_id` at `copy_move.rs:244`: when the accounts differ and either is a key account, `trc::event!(Security(Unauthorized), AccountId = src, Details = "zero-access: mail copy across accounts")` and `Err(ImapEvent::Error.into_err().details(ZA_MAIL_KEYLESS).code(ResponseCode::NoPerm).id(arguments.tag))`. JMAP `email_copy`, after the equal-ids check at `copy.rs:66`: the same condition gives the same trace and `Err(JmapEvent::Forbidden.into_err().details(ZA_MAIL_KEYLESS))`.
- Produces, sharing: IMAP `handle_set_acl`, after `get_acl_mailbox` at `acl.rs:270`: `if data.server.za_is_key_account(mailbox_id.account_id).await? { trace "zero-access: sharing refused on a key account's mailbox"; Err(ImapEvent::Error ... ZA_MAIL_KEYLESS ... NoPerm ... .id(tag)) }` (GETACL, MYRIGHTS and LISTRIGHTS stay). JMAP `mailbox_set_item`, inside `if has_acl_changes {` at `set.rs:612`: when `ctx.account_id` is a key account, `return Ok(Err(SetError::forbidden().with_description(ZA_MAIL_KEYLESS)))` after the trace.
- Produces, listing: `build_access_token`, after the `access_to` loop and before `let now = now();`: `for entry in access_to.iter_mut() { if self.za_is_key_account(entry.account_id).await.caused_by(trc::location!())? { entry.collections.remove(Collection::Mailbox); entry.collections.remove(Collection::Email); } } access_to.retain(|a| !a.collections.is_empty());` with a comment citing spec 6.3.
- Produces, in tests: `tests/src/za/mail/gates.rs`.

- [ ] **Step 1: Write the failing tests in `tests/src/za/mail/gates.rs`**

`pub async fn test(test: &mut TestServer)` with `keymail8@example.com`, `keymail9@example.com` (key) and `plainmail7@example.com`, `plainmail8@example.com` (plain), one delivered message each, in order:
1. `keyless_tokens_hit_the_dispatcher_gate`: with keymail8's Bearer token (OAuth) and over the WebSocket (`client.connect_ws()`): `Email/get` of the message, `Email/parse`, `SearchSnippet/get` with no filter, `Blob/get`, `Blob/upload`, `Email/import`, `Email/copy`, `EmailSubmission/set`, `Email/set` with a `create` each answer the method error `forbidden` with `ZA_MAIL_KEYLESS`; `Mailbox/get`, `Email/query` (`inMailbox`), `Email/queryChanges`, `Email/changes`, `Thread/get`, `Thread/changes`, `Email/set` with a `keywords` update and a `mailboxIds` update and a `destroy`, `Identity/get`, `VacationResponse/get`, `SieveScript/get`, `PushSubscription/get` each succeed; the trace subscriber (`Security(Unauthorized)`) saw `ZA_TRACE_MAIL_KEYLESS` with the account id and no method argument.
2. `cross_account_copies_are_refused`: keymail8's IMAP session `COPY 1 "Shared Folders/plainmail7@example.com/INBOX"` after plainmail7 shares its INBOX with `lrswi` to keymail8: `NO [NOPERM]`; `UID MOVE` likewise and the source stays; plainmail7's session `COPY` into keymail8's shared mailbox is impossible (not listed; the name resolves to `NO [TRYCREATE]`); JMAP `Email/copy` from keymail8 to plainmail7 (keymail8's client) answers `forbidden`; from plainmail7 to keymail8 (plainmail7's client, `accountId` keymail8) answers `forbidden`; `Blob/copy` both directions `forbidden`; between plainmail7 and plainmail8 the `Email/copy` succeeds (the control); plainmail7 removes the grant.
3. `sharing_is_refused_on_key_mailboxes`: keymail8's IMAP `SETACL INBOX plainmail7@example.com lr` answers `NO [NOPERM]`; `DELETEACL` likewise; JMAP `Mailbox/set` update with `shareWith` answers `notUpdated` `forbidden`; `Mailbox/set` create with `shareWith` answers `notCreated` `forbidden`; `GETACL INBOX` and `MYRIGHTS INBOX` still answer; plainmail7's `SETACL INBOX keymail8@example.com lr` succeeds (a plain account sharing to a key account) and keymail8's LIST shows `Shared Folders/plainmail7@example.com/INBOX` and `FETCH` of its message returns plaintext; plainmail7 removes the grant.
4. `rf_2_stale_grant`: `plainmail9@example.com` is created plain, shares its INBOX to plainmail7 with `lrswi` (IMAP SETACL from its own session), then is provisioned (`za_setup_token`, `za_setup`; it holds no message) and receives one message; plainmail7's IMAP session: `LIST "" "*"` does not show `Shared Folders/plainmail9@example.com/INBOX`; `SELECT "Shared Folders/plainmail9@example.com/INBOX"` answers `NO`; `APPEND "Shared Folders/plainmail9@example.com/INBOX" {..}` answers `NO`; JMAP `Mailbox/get` with `accountId` plainmail9 answers `forbidden` (no access) and the session object lists no such account; `Email/get` of plainmail9's message id from plainmail7's client answers `forbidden`; the trace carries `ZA_TRACE_MAIL_KEYLESS` for the stub read, never `DataCorruption`.
5. `copy_message_race_status` (moved from `fence.rs` step 5): the parked copy now ends in `forbidden`.
Cleanup as the module rule says.

- [ ] **Step 2: Run the tests to verify they fail**

Run: the za suite.
Expected: `keyless_tokens_hit_the_dispatcher_gate` fails (`Email/get` over the WebSocket returns the message).

- [ ] **Step 3: Implement the gate, the refusals and the listing filter**

Per the Interfaces block.

- [ ] **Step 4: Run the tests to verify they pass**

Run: the za suite (`websocket::test` and `gating::test` must still pass: the calendar gates are untouched); `jmap_tests` and `imap_tests` plain; the compile check; `cargo fmt` for `jmap`, `imap`, `common`, `tests`.
Expected: pass, `clean`.

- [ ] **Step 5: Commit**

```bash
git add crates/jmap/src/api/request.rs crates/jmap/src/email crates/jmap/src/mailbox/set.rs crates/imap/src/op/copy_move.rs crates/imap/src/op/acl.rs crates/common/src/auth/access_token.rs tests/src/za
git commit -m "Key-account mail gates: keyless JMAP methods, cross-account copies, sharing and listing"
```

### Task 8: Search, sort and snippet refusals

**Files:**
- Modify: `crates/imap/src/op/search.rs:263-285` (`query`, before the filter loop) and `:658-686` (the comparator loop)
- Create: `crates/jmap/src/email/za.rs` (`pub mod za;` in `crates/jmap/src/email/mod.rs`)
- Modify: `crates/jmap/src/email/query.rs:43-52` (before the filter loop) and `:277` (before the sort loop), `crates/jmap/src/email/snippet.rs:54-95` (after the terms are collected)
- Create: `tests/src/za/mail/search.rs`; Modify: `tests/src/za/mail/mod.rs`, `tests/src/za/mod.rs`
- Test: unit tests in `za.rs` and in `search.rs`'s test module; `tests/src/za/mail/search.rs`

**Interfaces:**
- Consumes: `imap_proto::protocol::search::{Filter, Sort, Comparator}`, `jmap_proto::object::email::{EmailFilter, EmailComparator}`, `jmap_proto::types::filter::Filter` (the `Property`/`And`/`Or`/`Not`/`Close` wrapper), `trc::JmapEvent::{UnsupportedFilter, UnsupportedSort}`, `ResponseCode::Cannot`, `common::za::ZA_MAIL_SEARCH_REFUSED`, `Server::za_is_key_account`.
- Produces, in `crates/imap/src/op/search.rs`: `fn za_filter_needs_content(filter: &Filter) -> bool` (true for `Bcc`, `Body`, `Cc`, `From`, `Header(..)`, `SentBefore`, `SentOn`, `SentSince`, `Subject`, `Text`, `To`; false for everything else, `And`/`Or`/`Not`/`End` included) and `fn za_sort_needs_content(sort: &Sort) -> bool` (true for `Cc`, `Date`, `From`, `DisplayFrom`, `Subject`, `To`, `DisplayTo`; false for `Arrival`, `Size`). In `query`, before the loop: `if self.server.za_is_key_account(mailbox.id.account_id).await? && (imap_filter.iter().any(za_filter_needs_content) || imap_comparator.iter().any(|c| za_sort_needs_content(&c.sort))) { return Err(ImapEvent::Error.into_err().details(ZA_MAIL_SEARCH_REFUSED).code(ResponseCode::Cannot)); }` (the callers add the tag; THREAD shares `query` and gets the same refusal for a content filter).
- Produces, in `jmap::email::za`: `pub fn filter_needs_content(filter: &EmailFilter) -> bool` (true for `Text`, `From`, `To`, `Cc`, `Bcc`, `Subject`, `Body`, `Header(..)`, `HasAttachment`, `SentBefore`, `SentAfter`, `_T(..)`; false for `InMailbox`, `InMailboxOtherThan`, `Before`, `After`, `MinSize`, `MaxSize`, `AllInThreadHaveKeyword`, `SomeInThreadHaveKeyword`, `NoneInThreadHaveKeyword`, `HasKeyword`, `NotKeyword`, `InThread`, `Id`), `pub fn sort_needs_content(comparator: &EmailComparator) -> bool` (true for `From`, `To`, `Cc`, `Subject`, `SentAt`; false for `ReceivedAt`, `Size`, `HasKeyword`, `AllInThreadHaveKeyword`, `SomeInThreadHaveKeyword`, `ThreadId`, `_T`), `pub fn first_refused_filter(filters: &[Filter<EmailFilter>]) -> Option<&EmailFilter>` (walks `Filter::Property` items), `pub fn first_refused_sort(sorts: &[Comparator<EmailComparator>]) -> Option<&EmailComparator>` (`request.sort` is `Option<Vec<..>>` upstream, `crates/jmap-proto/src/method/query.rs:26`: pass `request.sort.as_deref().unwrap_or_default()`). In `email_query`, after `account_id` is known and before the filter loop: `if self.za_is_key_account(account_id).await? { if let Some(f) = first_refused_filter(&request.filter) { return Err(JmapEvent::UnsupportedFilter.into_err().details(format!("{f}: {ZA_MAIL_SEARCH_REFUSED}"))); } if let Some(s) = first_refused_sort(&request.sort) { return Err(JmapEvent::UnsupportedSort.into_err().details(format!("{s}: {ZA_MAIL_SEARCH_REFUSED}"))); } }` (the `Display` of the filter prints its property name only; `za_method_error_for_trace` already replaces the traced details for key accounts). `Email/queryChanges` reaches `email_query` and inherits it. In `email_search_snippet`, after `terms` is built: `if !terms.is_empty() && self.za_is_key_account(account_id).await? { return Err(JmapEvent::UnsupportedFilter.into_err().details(format!("text: {ZA_MAIL_SEARCH_REFUSED}"))); }`.
- Produces, in tests: `tests/src/za/mail/search.rs`.

- [ ] **Step 1: Write the failing unit tests**

In `crates/jmap/src/email/za.rs`: a table test asserting `filter_needs_content` and `sort_needs_content` for every variant of the two enums (construct each with a dummy value), so a new upstream variant fails the match's exhaustiveness at compile time and the table at test time. In `crates/imap/src/op/search.rs`'s test module (create it if absent): the same for `Filter` and `Sort`.

- [ ] **Step 2: Write the failing tests in `tests/src/za/mail/search.rs`**

`pub async fn test(test: &mut TestServer)` with `keymail10@example.com` (key) and `plainmail10@example.com` (plain), each with four delivered messages of distinct sizes, arrival times (`received_at` through Email/import with `receivedAt`), flags and subjects, in order:
1. `imap_served_keys`: on the key account, each of `ALL`, `1:2`, `UID 1:2`, `ANSWERED`, `UNANSWERED`, `DELETED`, `UNDELETED`, `DRAFT`, `UNDRAFT`, `FLAGGED`, `UNFLAGGED`, `SEEN`, `UNSEEN`, `KEYWORD $Label1`, `UNKEYWORD $Label1`, `RECENT`, `NEW`, `OLD`, `MODSEQ 1`, `EMAILID <id>`, `THREADID <id>`, `BEFORE 1-Jan-2030`, `ON 1-Jan-2020`, `SINCE 1-Jan-2020`, `OLDER 1`, `YOUNGER 100000`, `LARGER 100`, `SMALLER 100000`, `UIDAFTER 1`, `UIDBEFORE 5` answers `* SEARCH` and OK with the same ids the plain account returns; `SORT (ARRIVAL) UTF-8 ALL`, `SORT (REVERSE SIZE) UTF-8 ALL` likewise; `THREAD REFERENCES UTF-8 ALL` answers four singletons.
2. `imap_refused_keys`: each of `BCC x`, `BODY x`, `CC x`, `FROM x`, `HEADER X-Private x`, `SENTBEFORE 1-Jan-2030`, `SENTON 1-Jan-2020`, `SENTSINCE 1-Jan-2020`, `SUBJECT x`, `TEXT x`, `TO x` answers `NO [CANNOT]` with `ZA_MAIL_SEARCH_REFUSED` and no `* SEARCH` line; `OR FROM x SEEN` (mixed) likewise; `SORT (SUBJECT) UTF-8 ALL`, `(DATE)`, `(FROM)`, `(DISPLAYFROM)`, `(TO)`, `(DISPLAYTO)`, `(CC)` likewise; `THREAD REFERENCES UTF-8 FROM x` likewise; the plain account answers every one of them with results (the control); the connection stays open after each refusal; the trace subscriber saw no `Security` event for these (they are not security events) and no search term in `ImapEvent::Error`'s traced details (Task 9 pins the redaction; here assert the event is `ImapEvent::Error`).
3. `jmap_served_filters_and_sorts`: `Email/query` with each of `inMailbox`, `inMailboxOtherThan`, `hasKeyword`, `notKeyword`, `allInThreadHaveKeyword`, `someInThreadHaveKeyword`, `noneInThreadHaveKeyword`, `inThread`, `id`, `before`, `after`, `minSize`, `maxSize` returns the same ids as the plain account; sorts `receivedAt`, `size`, `hasKeyword`, `allInThreadHaveKeyword`, `someInThreadHaveKeyword` likewise; `collapseThreads: true` returns four; `Email/queryChanges` with `inMailbox` and `receivedAt` sort returns the changes.
4. `jmap_refused_filters_and_sorts`: each of `text`, `from`, `to`, `cc`, `bcc`, `subject`, `body`, `header: ["X-Private"]`, `hasAttachment: true`, `sentBefore`, `sentAfter` answers `unsupportedFilter` whose description starts with the property name and contains `ZA_MAIL_SEARCH_REFUSED`; an `AND` of `inMailbox` and `from` likewise; sorts `from`, `to`, `cc`, `subject`, `sentAt` answer `unsupportedSort`; `Email/queryChanges` with `from` answers `unsupportedFilter`; `SearchSnippet/get` with `filter: {text: "x"}` answers `unsupportedFilter`, with `{subject: "x"}` and `{body: "x"}` likewise, and with `{inMailbox: ..}` (no term) answers snippets whose `subject` and `preview` are null; the plain account answers every one of them with results (the control).
5. `visible_field_document`: after `wait_for_tasks`, the search records of the key account (scanned as `leak::scan` scans `SUBSPACE_SEARCH_INDEX`) hold Email `Document`-layout entries for the four messages and no `Term`-layout entry; the plain account has `Term` entries (the control).
Cleanup as the module rule says.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p jmap email::za`, `cargo test -p imap`, the za suite.
Expected: compile errors; `imap_refused_keys` fails (FROM returns results).

- [ ] **Step 4: Implement the predicates and the three checks**

Per the Interfaces block.

- [ ] **Step 5: Run the tests to verify they pass**

Run: the unit tests; the za suite; `imap_tests` and `jmap_tests` plain; the compile check; `cargo fmt` for `imap`, `jmap`, `tests`.
Expected: pass, `clean`.

- [ ] **Step 6: Commit**

```bash
git add crates/imap/src/op/search.rs crates/jmap/src/email tests/src/za
git commit -m "Search, sort and snippet requests on sealed fields are refused with a clear error"
```

### Task 9: Trace redactions for key-account mail

**Files:**
- Modify: `crates/imap/src/core/client.rs:22-32` (raw input), `crates/imap/src/core/session.rs:237-249` (`Session::write_bytes`), `:263-295` (`write_error`, `traced_error`), `:298-307` (`SessionData::write_bytes`), `:321-330` (`SessionData::write_error`), `crates/imap/src/op/fetch.rs:636`, `crates/imap/src/op/store.rs:203,445`
- Modify: `crates/pop3/src/client.rs:20-27,85-91`, `crates/pop3/src/session.rs:159-176,192-215`
- Modify: `crates/smtp/src/core/mod.rs:63-99,175,299` (`za_key_rcpt: bool`), `crates/smtp/src/inbound/rcpt.rs:194-264` (the flag after each accepted address), `crates/smtp/src/inbound/session.rs:486-489,543-563,655-686` (`reset`, `read`, `has_data_command`), `crates/smtp/src/inbound/data.rs:546-551` (the autolearn sample skipped for a key recipient, spec 6.1 step 4) and `:741` (the flag cleared with `rcpt_to`), `crates/smtp/src/inbound/milter/client.rs:25-75,313,333` (`redact: bool` on `MilterClient`), `crates/smtp/src/inbound/milter/message.rs:158` (`connect_and_run` passes `self.data.za_key_rcpt`), `crates/smtp/src/inbound/spam.rs:57` (`za_redact` on the input)
- Modify: `crates/spam-filter/src/lib.rs:26-60` (`SpamFilterInput.za_redact: bool`), `crates/spam-filter/src/analysis/llm.rs:46`, `crates/spam-filter/src/modules/dnsbl.rs:82,114,141,166,185`, `crates/spam-filter/src/analysis/rules.rs:32,51,68,88,108,137,169,186`
- Modify: `crates/jmap/src/vacation/set.rs:437-440`
- Create: `tests/src/za/mail/traces.rs`, `tests/src/utils/milter.rs` (the mock milter lifted from `tests/src/smtp/inbound/milter.rs:680-838` into a shared helper, re-exported there); Modify: `tests/src/za/mail/mod.rs`, `tests/src/za/mod.rs`
- Test: `tests/src/za/mail/traces.rs`, unit tests for `has_data_command` in `session.rs`

**Interfaces:**
- Consumes: Task 2's `za_key_session()` on IMAP `Session`/`SessionData` and POP3 `State`; `trc::Value::redacted()`; `trc::Error::redact(&[Key])`; `is_auth_input`/`has_auth_command`'s shape; `AccountInfo.account.is_key_account()`; `Server::{account_id_from_email, za_is_key_account}`; `Server::za_eval_if` (`crates/common/src/expr/eval.rs:36`); `SpamFilterContext.input`.
- Produces, the inventory (spec 13 item 12), with the decision per site:

  | Site | Key | In a key-account session or transaction |
  |---|---|---|
  | `imap/core/client.rs:27` raw input | `Contents` | `[redacted]` when `NotAuthenticated` or `za_key_session()` |
  | `imap/core/session.rs:244` `Session::write_bytes` | `Contents` | the same |
  | `imap/core/session.rs:306` `SessionData::write_bytes` | `Contents` | `[redacted]` when `za_key_session()` |
  | `imap/core/session.rs:286-291` `traced_error` | `Details`, `Id` of BAD `ImapEvent::Error` | redacted when `NotAuthenticated` or `za_key_session()` |
  | `imap/core/session.rs:321-330` `SessionData::write_error` | the same keys | the same redaction (today it has none) |
  | `imap/op/fetch.rs:636` `ImapEvent::Fetch` | `Details` (attribute Debug, header names) | `[redacted]` |
  | `imap/op/store.rs:203,445` `ImapEvent::Store` | `Details` (keywords) | `[redacted]` |
  | `pop3/client.rs:24` raw input, `pop3/session.rs:166` raw output | `Contents` | `[redacted]` when `NotAuthenticated` or `za_key_session()` |
  | `pop3/client.rs:87` parse errors | `Type = BAD` marking | also when `za_key_session()` (so `write_err` redacts `Details`/`Id`) |
  | `smtp/inbound/session.rs:554` raw input | `Contents` | `[redacted]` when `za_key_rcpt` and (`State::Data | Bdat | SkipData` or `has_data_command(partial ⧺ bytes)`), or when `za_key_sender()` (after authentication as a key account) |
  | `smtp/inbound/milter/client.rs:313,333` `MilterEvent::Write`/`Read` | `Contents` | `[redacted]` when `client.redact` |
  | `spam-filter/modules/dnsbl.rs:141,166,185` `Spam(Dnsbl)`/`DnsblError` | `Hostname` | `[redacted]` when `input.za_redact` |
  | `spam-filter/analysis/llm.rs:46` `Ai(LlmResponse)` | `Details` | `[redacted]` when `input.za_redact` |
  | `spam-filter/analysis/rules.rs` and `dnsbl.rs` `eval_if` (10 sites) | `Eval(Result)` | `za_eval_if` when `input.za_redact` (a local `fn eval<'x, R, V>(ctx, if_block, resolver) ` choosing the variant) |
  | `email/message/ingest.rs:697-717,221` `MessageIngest` | `MessageId` | `[redacted]` (Task 3 sets `message_id`) |
  | `smtp/inbound/data.rs:546-551` the autolearn global sample (`spool.rs:510-553`, which stores the subject and the queue blob) | the sample | `train_spam = None` when `za_key_rcpt` (spec 6.1 step 4) |
  | `jmap/vacation/set.rs:437-440` | `Reason` | the line and column of `CompileError` only, every account: `.reason(format!("compile error at line {}, column {}", err.line_num, err.line_pos))` (the field names as sieve-rs 0.7.3 names them) |
  | `smtp/inbound/session.rs:345` `UnsupportedParameter`, `rcpt.rs` `To` keys, `spool.rs` `From`/`To`, `Spam(Classify)` `Result` | envelope and parameter strings | unchanged: transit metadata (spec 2) |
  | `ImapEvent::{Append, CreateMailbox, DeleteMailbox, RenameMailbox, Select, Status, Subscribe, GetAcl, MyRights, SetAcl, ListRights, GetQuota, List, Lsub}` `MailboxName`/`Details` | mailbox names, rights, quota numbers | unchanged: visible by structure |
  | `ImapEvent::Error` values with `.details(format!(..))` in `create.rs`, `rename.rs`, `search.rs:491,502`, `expunge.rs:69`, `fetch.rs:522` | mailbox names, ids, sequence sets, part numbers | unchanged: no content |
  | MTA-hook events (`hooks/message.rs:56,153`) | ids, serde error text | unchanged: no header reaches them |

- Produces, in SMTP: `SessionData::za_key_rcpt: bool` (doc: "Mail spec 10: a recipient of the current transaction is a key account; the DATA and BDAT reads are redacted."), set in `rcpt.rs` after every arm that leaves an address in `rcpt_to` (`Accept`, `Rewrite`, each `Expand` member) by `self.za_note_key_rcpt(&address_lcase).await` (`account_id_from_email(address, false)` then `za_is_key_account`, errors ignored as `false`... no: as `true`, fail closed: an error redacts), cleared in `reset()` and where `rcpt_to` is taken at `data.rs:741`; `fn za_key_sender(&self) -> bool` on `SessionData` (`authenticated_as.as_ref().is_some_and(|a| a.account.is_key_account())`); `fn has_data_command<'x>(bytes: impl Iterator<Item = &'x u8>) -> bool` next to `has_auth_command` (a case-insensitive `DATA` followed by CR or LF, or `BDAT` followed by a space or tab, at a line start or after whitespace, anywhere in the joined bytes).
- Produces, in tests: `tests/src/za/mail/traces.rs` and the shared mock milter.

- [ ] **Step 1: Write the failing unit test for `has_data_command`** in `session.rs`'s test module: true for `b"DATA\r\n"`, `b"bdat 12\r\nxx"`, `b"RCPT TO:<a@b>\r\nDATA\r\n"`, `b" data\n"`; false for `b"RCPT TO:<data@b>\r\n"`, `b"DATAX\r\n"`, `b"NOOP\r\n"`, `b"UPDATA\r\n"`.

- [ ] **Step 2: Write the failing tests in `tests/src/za/mail/traces.rs`**

`pub async fn test(test: &mut TestServer)` with `keymail11@example.com` (key) and `plainmail11@example.com`; the module configures a milter on 9332 at the `Data` stage through `MtaMilter` (as `milter_session` does) and starts the shared mock; the subscriber pattern of `raw_input.rs` with interests `Imap(RawInput|RawOutput|Error|Fetch|Store)`, `Pop3(RawInput|RawOutput|Error)`, `Smtp(RawInput|RawOutput)`, `Milter(Write|Read)`, `Spam(Dnsbl|DnsblError|Classify)`, `MessageIngest(Ham|Spam|JmapAppend|ImapAppend)`, `Eval(Result)`, `Store(DataCorruption)`; canaries `hdr-name-canary` (a header name `X-Hdr-Name-Canary`), `hdr-value-canary`, `subject-canary`, `body-canary`, `msgid-canary@example.com`. In order:
1. `delivery_leaves_no_content_in_events`: deliver a canary message to the key account over LMTP; drain the subscriber; no captured event of any type contains a canary; `Milter(Write)` events exist for the transaction with `Contents == "[redacted]"`; `MessageIngest(Ham)` exists with `MessageId == "[redacted]"`; the same delivery to the plain account shows `HEADER (X-Hdr-Name-Canary: ..)` in a `Milter(Write)` and the Message-ID in `MessageIngest` (the control).
2. `smtp_submission_by_a_key_account_is_redacted`: over the submission listener (4465, implicit TLS, the `Conn` helper), `AUTH PLAIN` as keymail11, `MAIL FROM`, `RCPT TO` plainmail11, `DATA`, the canary message, `.`: every `Smtp(RawInput)` after the authentication is `[redacted]`; the same session by plainmail11 shows the body lines (the control); a BDAT variant whose `BDAT 123` line and chunk arrive in one write to the inbound LMTP listener for the key recipient shows `[redacted]` for that read, and the envelope commands before it (`LHLO`, `MAIL FROM`, `RCPT TO`) stay readable.
3. `imap_session_is_redacted`: keymail11's IMAP session: `SELECT INBOX`, `FETCH 1 (BODY[HEADER.FIELDS (X-Hdr-Name-Canary)])`, `STORE 1 +FLAGS ($kw-canary)`, `APPEND INBOX {..}` of a canary message, `SEARCH FROM x` (refused), a malformed command `FETCH 1 (BODY[HEADER.FIELDS (unterminated`: every `Imap(RawInput|RawOutput)` after the login is `[redacted]`; `Imap(Fetch)`'s and `Imap(Store)`'s `Details` are `[redacted]`; the BAD error's traced `Details` and `Id` are `[redacted]`; the plain account's session shows the header name in `Imap(Fetch)`, the keyword in `Imap(Store)`, the FETCH literal in `RawOutput` and the malformed command's text in the error (the control); the client still received the full responses (the FETCH answer holds the canary).
4. `pop3_session_is_redacted`: keymail11's POP3 session: `RETR 1`, `TOP 1 0`, `AUTH BOGUS` after login (the "Unsupported mechanism" parse error): every `Pop3(RawInput|RawOutput)` after login is `[redacted]`, the error's `Details` is `[redacted]`; the plain account's shows the message (the control); the client received the message.
5. `vacation_compile_error_reason_is_fixed`: a unit test in `crates/jmap/src/vacation/set.rs`'s test module compiles a deliberately invalid script through the same `sieve::Compiler` the handler uses, builds the error exactly as the handler does, and asserts the `Reason` matches `compile error at line \d+, column \d+` and carries none of the script's text; the integration half, for the plain account, sends `VacationResponse/set` with a subject holding `vacation-canary` and a `textBody` holding a raw NUL byte and an unbalanced `"`: if the server answers an error, its traced `Reason` matches the pattern and no captured event carries `vacation-canary`; if the generated script compiles anyway, the case records that in the task report and the unit test stands as the pin.
6. `autolearn_skips_key_transactions`: the module's setup registers `SpamClassifier { auto_learn_spam_trap: true, .. }`, the spam-trap lookup `spamtrap@*` in namespace `spam-traps` (as `imap_tests` does at `tests/src/imap/mod.rs:146-152, 206-212`) and `MtaStageData { enable_spam_filter: true }`; one LMTP transaction to `["spamtrap@example.com", "keymail11@example.com"]` writes no global sample (`spam_samples(test)` unchanged, no `Spam(TrainSampleAdded)` event) while the same transaction to `["spamtrap@example.com", "plainmail11@example.com"]` writes one (the control); after `wait_for_delivery` the key account's copy is sealed.
Cleanup: remove the milter, the classifier and the trap settings, stop the mock, delete messages, revoke nothing (no app passwords used).

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p smtp has_data_command`, the za suite.
Expected: compile error; `delivery_leaves_no_content_in_events` fails on the milter `Write` contents.

- [ ] **Step 4: Implement the redactions**

Per the inventory, one row at a time, re-running `traces::test` after the SMTP, IMAP and POP3 groups. The milter `redact` flag travels from `run_milters` through `connect_and_run` into `MilterClient::connect`'s returned struct. `raw_input::test` must keep passing (pre-auth masking is untouched).

- [ ] **Step 5: Run the tests to verify they pass**

Run: the za suite; `cargo test -p tests smtp` (the milter and hook tests, with the mock moved); `imap_tests` plain; the compile check; `cargo fmt` for `imap`, `pop3`, `smtp`, `spam-filter`, `jmap`, `tests`.
Expected: pass, `clean`.

- [ ] **Step 6: Commit**

```bash
git add crates/imap/src crates/pop3/src crates/smtp/src crates/spam-filter/src crates/jmap/src/vacation/set.rs tests/src
git commit -m "Traces of key-account mail carry no content: raw I/O, milter, spam-filter and IMAP command events"
```

### Task 10: Key-mode suites, the leak extension, docs, CI and the outcome note

**Files:**
- Create: `tests/src/imap/za_variants.rs`, `tests/src/jmap/za_variants.rs`
- Modify: `tests/src/imap/mod.rs:61-109,218-285`, `tests/src/jmap/mod.rs:41-77,186-226` (key-mode provisioning and dispatch), `tests/src/webdav/mod.rs:263-267` (`key_accounts_mode` moved to `tests/src/utils/za.rs` and re-exported)
- Modify: `tests/src/za/leak.rs` (`check_email`, the Email index rule, `EXTRA_CANARIES`, `pub async fn test_mail`), `tests/src/za/mod.rs` (`leak::test_mail(&mut test).await;` after `leak::test_pins`), `tests/src/utils/za.rs` (`plant_plain_message` from Task 5 if not yet there)
- Modify: `docs/zero-access/manual-checklist.md`, `CLAUDE.md` (the invariant line at `:67`, the status paragraph, the merge hotspots, the za suite paragraph), `docs/superpowers/plans/README-dev.md:29`, `.github/workflows/test.yml:66-119`
- Create: `docs/superpowers/plans/2026-10-20-zero-access-plan13-outcome.md`

**Interfaces:**
- Consumes: everything above; `key_accounts_mode()`; `create_key_user_account`; the sub-module flags of spec 13 item 10 (IMAP: `search` S, `fetch` S, `store` S, `thread` S, `acl` A+X, `antispam` T+A+X, `messagelimit` UID-only; JMAP: `mail::query` S, `search_snippet` S, `query_changes` S, `copy` X, `thread_get` S, `thread_merge` S, `acl` A+X, `push_subscription` P, `compliance` mixed; the rest run unchanged, `mail::get`'s golden-file comparison being the real guard of Task 5's view).
- Produces: key-mode dispatch in both suites, as `webdav/mod.rs:205-247` does: `if key_accounts_mode() { za_variants::<name>(&test, ..).await } else { <name>::test(..).await }` for the flagged sub-modules; variants assert the refusals of Tasks 7 and 8 and the designed outcomes (singleton threads, no samples, state-only push) and otherwise run the upstream steps; `compliance::test` is skipped in key mode with a printed note (its raw-JSON email query and snippet runs would need a parallel suite; the outcome note records it); the `mail::copy` module, which copies between accounts 1 and 2 by id, becomes the refusal variant; `thread_merge`'s server-level `email_ingest` calls use `IngestSource::Smtp` with a keyless token and so seal under `pk` (the variant asserts the singletons and that the task queue drained).
- Produces, in `leak.rs`: `check_email(archive, what, violations, counters)` on `SUBSPACE_PROPERTY` keys `account_id | Collection::Email | field 71 | document_id` of the scanned account: a decoded `MessageMetadata` whose `raw_headers` does not start with `MAIL_FORMAT_V1` is a violation (`"{what}: plaintext metadata archive"`), a stub's `preview` must be empty and its root part empty (counts `scan.stubs`); on `SUBSPACE_INDEXES` keys of property 90 (Threading) for the account: a violation; on `SUBSPACE_SEARCH_INDEX` Email entries of the account: a `Term`-layout key is a violation, a `Document`-layout value decoded with the store's own search-document deserializer (the one `crates/store/src/search/local.rs` reads with) must hold only `ReceivedAt` and `Size`; on `SUBSPACE_SPAM_SAMPLES`: any record with `account_id == Some(scanned)` is a violation; on `SUBSPACE_QUEUE_MESSAGE` and `SUBSPACE_BLOB_LINK`: a temporary link held by the account after `wait_for_delivery` is a violation. `EXTRA_CANARIES` gains `mail-sub` (the ≤15-byte subject, `"mail-sub canary"` is 15 bytes), `mailfrom-canary`, `mailname-canary`, `mailrcpt-canary`, `mailmsgid-canary`, `mailbody-canary`, `mailattach-canary`, `mailattname-canary`, `mailhdrname-canary`, `mailhdrval-canary`, `mailpreview-canary`. `pub async fn test_mail(test: &mut TestServer)`: a key account `keymail12@example.com` receives over LMTP a message carrying every canary (subject `mail-sub canary`, `From: mailname-canary <mailfrom-canary@example.com>`, `To` a `mailrcpt-canary+keymail12@example.com` alias of the account, `Message-ID`, a custom header `X-Mailhdrname-Canary: mailhdrval-canary`, a text body starting with `mailpreview-canary` and holding `mailbody-canary`, a base64 attachment named `mailattname-canary.txt` holding `mailattach-canary`) and APPENDs a second one over IMAP; the subscriber captures IMAP, POP3 and SMTP raw and command events of a session that FETCHes, RETRs and submits the message; then `scan(test, id)` has no violations, `scan.stubs == 2`, `scan.emails == 2` (the cached-message loop opens nothing: it reads `fetch_email`'s raw bytes, which are ciphertext; the canary search over them is the point), no captured event holds a canary; positive controls: a keyword `$mailkw-canary` set on the message and a mailbox `mailbox-canary` are found in the scan (`scan.violations` must not list them: they are visible by structure, so the scanner is asked for `scan.found` entries instead: add `found: Vec<String>` to `Scan` for allowlisted hits); negative controls: `plant_plain_message` with a canary is flagged `plaintext metadata archive` and its blob's canary found; a planted plaintext metadata archive alone (a stub rewritten with `raw_headers` = the plaintext header block, via a direct field-71 write) is flagged.
- Produces, docs: the checklist gains a `## Mail` section with the items of spec 11's manual checklist (Apple Mail macOS and iOS, Thunderbird, FairEmail or Thunderbird for Android; add with the password and an app password; receive, read, reply with a Sent copy, forward an attachment, move, flag, delete, create and rename a folder, server search and the local search, IDLE push, reconnect after a password change, a TOTP account through an app password, vacation on and off); `CLAUDE.md:67` becomes "`Server::authenticate` returns keyless tokens on every protocol. `SessionKeys` are attached by the HTTP layer for one request, and by the IMAP and POP3 login handlers for the life of the authenticated connection (mail spec 3.3); WebSocket, EventSource, ManageSieve and SMTP sessions never hold them. Background code never needs a key; if a path would, gate it."; README-dev's line 29 is amended the same way; CLAUDE.md's invariants list gains one bullet summarising mail's 24-32 in the shape of the key-pinning bullet (stub plus sealed blob whatever wrote it, a session only with keys, no threading index, term, sample or push content, plaintext only in the queue and a failed ordinary ingest's staged blob, delivery never in the clear, traces without content, no raw read of a sealed source, the ordinary-writer assertion as an invariant 9 exception), the "Mail-protocol traces" bullet gains the whole-session rule, and CLAUDE.md gains a "Status, release 3" paragraph, the mail merge hotspots (`crates/email/src/message/{ingest,copy}.rs`, `crates/email/src/message/index/metadata.rs`, `crates/imap/src/op/{authenticate,fetch,search,copy_move,acl,store}.rs`, `crates/imap/src/core/{mod,client,session}.rs`, `crates/pop3/src/{client,session}.rs`, `crates/pop3/src/op/{authenticate,fetch}.rs`, `crates/jmap/src/email/{get,query,snippet,copy,set}.rs`, `crates/jmap/src/blob/{download,upload,copy}.rs`, `crates/jmap/src/file/{set,copy}.rs`, `crates/jmap/src/submission/set.rs`, `crates/jmap/src/mailbox/set.rs`, `crates/jmap/src/api/request.rs`, `crates/common/src/auth/access_token.rs`, `crates/services/src/task_manager/index.rs`, `crates/services/src/state_manager/{push,email_push}.rs`, `crates/smtp/src/inbound/{session,rcpt,data,spam}.rs`, `crates/smtp/src/inbound/milter/{client,message}.rs`, `crates/spam-filter/src/lib.rs` and its `eval_if` sites, `crates/http/src/request.rs`), and the za suite paragraph lists the mail modules; CI's `zero-access` job gains two steps after the key-mode CalDAV run: `IMAP Tests (key accounts)` and `JMAP Tests (key accounts)` running `cargo test -p tests imap::imap_tests -- --nocapture` and `jmap::jmap_tests` with `ZA_KEY_ACCOUNTS: "1"`.
- Produces, the outcome note: the rulings (the decisions block of this plan's header and any made during execution), the mutation table (Tasks 3, 4, 5 kill checks plus the ones below), the deferred findings (compliance in key mode, Bearer/OAuth wraps for release 5, the PostgreSQL/MySQL lock as its own plan, the product copy for Circulo: the visible-and-sealed list, the transit and residency limits, the search behaviour, the spam-training note), the facts for release 4 (where the iMIP gate sits in the sealed branch, which helpers a sealed pending item from ingest would reuse), and the open spec question, if any, Jay should rule on.

- [ ] **Step 1: Key-mode provisioning and dispatch**

`key_accounts_mode()` moves to `tests/src/utils/za.rs` (re-exported from `webdav/mod.rs`). In both suites the user loop uses `create_key_user_account` under key mode (groups and the admin plain, as webdav does). Run each suite in key mode once before writing variants: `ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests imap_tests -- --nocapture`, then `jmap_tests`; classify every failure: a designed refusal or outcome (spec 6.3, 6.4, 8, 9) gets a variant; anything else is a bug in Tasks 1-9 and is fixed there (a follow-up commit on that task's area, listed in the report). Write the variants: `tests/src/imap/za_variants.rs` with `search`, `fetch` (the saved-search step replaced by a `SEARCH RETURN (SAVE) SEEN`), `store` (likewise), `thread` (singletons), `acl` (sharing and cross-account steps assert `NO [NOPERM]`; the plain-group steps run), `antispam` (no sample for the key account, the trap and the plain-account steps run); `tests/src/jmap/za_variants.rs` with `query` (every content filter and sort asserts `unsupportedFilter`/`unsupportedSort`; the served ones run against the imported corpus), `search_snippet`, `query_changes`, `copy` (refusal), `thread_get` and `thread_merge` (singletons), `acl` (sharing and cross-account steps assert `forbidden`; shares from plain accounts to key accounts run), `push_subscription` (state-only), and the `compliance` skip.

- [ ] **Step 2: Run both suites in key mode**

Run: the two key-mode commands, then plain `imap_tests` and `jmap_tests`.
Expected: all four pass; `mail::get`'s golden files pass unchanged in key mode (the view equals upstream's archive).

- [ ] **Step 3: The leak extension**

`check_email`, the index, threading, sample and link rules, the canaries, `test_mail`, the `found` list. Run the za suite twice: with the default blob store and with `BLOB_STORE=FileSystem`.
Expected: `test_mail` passes with `stubs == 2`; `leak::test`, `test_drain`, `test_pins`, `test_index_control` unchanged.

- [ ] **Step 4: Mutation runs** (each against a commit, reverted, none committed)

| Mutation | Must fail |
|---|---|
| `za_metadata_view` returns `Borrowed` for a stub | `reads::offsets_round_trip`, key-mode `mail::get` |
| `email_ingest` reuses `params.blob_hash` for a key account | `writes::email_import_reseals_the_upload`, `leak::test_mail` (a plaintext upload blob linked as the message) |
| the `find_thread_id` skip removed | `leak::test_mail` (a `Threading` key) |
| `train_spam = None` removed | `delivery::same_message_to_a_key_and_a_plain_account`, `leak::test_mail` |
| the dispatcher gate removed for `Email/get` | `gates::keyless_tokens_hit_the_dispatcher_gate` |
| `za_filter_needs_content` returns false for `From` | `search::imap_refused_keys`, the unit table |
| the `access_to` filter removed | `gates::rf_2_stale_grant` (LIST shows the mailbox) |
| `/jmap/upload` 403 removed | `uploads::upload_without_keys_is_refused` |
| `za_key_rcpt` never set | `traces::delivery_leaves_no_content_in_events` |
| `SessionData::write_bytes` redaction removed | `traces::imap_session_is_redacted` |
| the index task gate removed | `search::visible_field_document`, `leak::test_mail` (term entries) |
| the push conversion condition without the key check | `reads::push_is_state_only` |
| the recheck's `za_vault_delete` call removed | `fence::reverse_race_setup_token_aborts` (a record remains) |

- [ ] **Step 5: Docs, CI and the outcome note**

The checklist section, CLAUDE.md, README-dev, `test.yml`, and the outcome note with the kill and mutation lines, the key-mode classification of step 1, the deferred findings and the release 4 facts.

- [ ] **Step 6: Final verification**

Run: the compile check; the product build (`cargo build --release -p stalwart --no-default-features --features rocks`, with the thin-LTO prefix for speed); `cargo fmt` checks for every touched crate; the za suite (both blob stores); `webdav_tests` plain and key mode; `imap_tests` and `jmap_tests` plain and key mode; `cargo test -p tests smtp`; `cargo test -p vault -p groupware -p email -p common -p jmap -p imap`.
Expected: all pass, `clean`.

- [ ] **Step 7: Commit**

```bash
git add tests/src docs CLAUDE.md .github/workflows/test.yml
git commit -m "Key-mode mail suites, the leak-test mail extension, the checklist, docs and CI for release 3"
```
