# Zero-access calendar: plan 8 outcome

Written 2026-10-13 at the end of plan 8 (`2026-10-13-zero-access-8-sealed-items.md`).
Read it before plans 9 and 10.

## Where things stand

- Commits on `release2-invitations-plan` (cut from main at `8a6ae34a`, not pushed):
  - the spec amendment `971008e1`, the plan `933d4e2a` and the CLAUDE.md fix `dabae7af`;
  - the implementation `1b5a3c2e..eca18e12` (Tasks 1 to 8), then this note.
- The suites run in Task 9 Step 2, in order, all on the committed tree, with no reruns:
  - `cargo test -p vault -p groupware`: groupware 69 passed, vault 30 passed.
  - `cargo test -p common auth::vault`: 14 passed.
  - `za_tests`: 1 passed in 51.34s (leak scan: 1072 records, 58 archives, 5 blobs, 3 items, 1 email).
  - `za_tests` with `BLOB_STORE=FileSystem`: 1 passed in 47.22s (leak scan: 1058 records, 3 items, 1 email).
  - `webdav_tests` with `ZA_KEY_ACCOUNTS=1`: 1 passed in 20.85s. This includes the key-account scheduling variant, which still asserts an empty inbox for jane.
  - `webdav_tests`: 1 passed in 16.63s. `cal_itip` passed on the first run.
  - `cargo build --release -p stalwart --no-default-features --features rocks`: finished in 16m 12s.
    - No warning in `vault`, `groupware`, `dav` or `common/src/auth/vault.rs`.
    - The filter `common/src/auth` matches four upstream warnings in files plan 8 does not touch: `permissions.rs:48`; `authentication.rs:661`, `:698`, `:720`.
    - Crate warning totals, all upstream's: `store` 1, `common` 18 (those four included), `jmap` 3, `services` 2.
  - `git status --short crates/` printed nothing.
- What now exists:
  - the `pk` wrap;
  - the private key in session keys;
  - token pairs in format 2 bundles, carried by the update and PROPPATCH seal sites;
  - inbox items sealed in both states;
  - pending items hidden, drained items unsealed.
- What does not exist yet:
  - nothing sends, drains or sweeps;
  - `ItipSendStatus::KeyAccount` is unchanged;
  - the create branch of PUT passes `None` pairs.

## Names settled (spec 13)

No task changed a name; the table is the plan's, unchanged.

| Name | Value | Where |
|---|---|---|
| Wrap type of a `pk` envelope | `WRAP_PK: u8 = 2`, next to `WRAP_MK` | `crates/groupware/src/calendar/seal/tree.rs` |
| `pk` envelope bytes | `policy version \|\| 0x02 \|\| E (32) \|\| wrapped DEK (72)` | spec 3.1, 9 |
| HKDF label | `LABEL_PK = b"za/v1/pk"`; HKDF info is `LABEL_PK \|\| E \|\| P` (64 bytes appended) | `crates/vault/src/keys.rs` |
| Wrapped-DEK purpose | `AAD_PK_DEK = "pk-dek"`; `aad` adds `za/v1\|` itself; the full associated data is the "Pending item key envelope" row below | `keys.rs` |
| `X-ZA-EXTRA` format with pairs | `FORMAT_V2: u8 = 2`, plaintext `ExtraV2` (Extra's fields plus `pairs: Vec<(String, [u8; 16])>`); its associated data ends with `\|` and `0x02` | `seal/tree.rs`, `seal/event.rs` |
| Token pair type | `TokenPairs` (Zeroizing storage, silent `Debug`), `TOKEN_LEN = 16`, `tokens_equal` (constant time) | new `seal/token.rs` |
| Inbox item scope | `ITIP_SCOPE = "itip"` for `tree_aad`, pending and drained | new `seal/item.rs` |
| Drained item key envelope | event-key shape `aad(AAD_ITIP_KEY, owner) \|\| "\|" \|\| uid`, `AAD_ITIP_KEY = "itip-key"`, wrapped under the owner's EWK (MK-derived) | `seal/item.rs` |
| Pending item key envelope associated data | `aad(AAD_PK_DEK, recipient) \|\| "\|" \|\| uid` (event-key shape; `AAD_PK_DEK = "pk-dek"`) | `seal/item.rs` |
| Pending item `X-ZA-EXTRA` associated data | `aad("itip-extra", recipient) \|\| "\|" \|\| uid` | `seal/item.rs` |
| Reply token property | `RSVP_TOKEN_PROP = "X-ZA-RSVP-TOKEN"`, base64url without padding, visible | `seal/item.rs` |
| Item root carrier order | last entries: `X-ZA-SEALED` (if any), then `X-ZA-RSVP-TOKEN` (reply) or `X-ZA-EXTRA` (REQUEST, CANCEL), at most one, then `X-ZA-KEY`; anywhere else a stray carrier | `seal/item.rs` |
| Pending flag | `EVENT_NOTIFICATION_ZA_PENDING: u16 = 1 << 2` | `crates/groupware/src/calendar/mod.rs` |
| Where the seal step gets the pairs | `seal_event(.., pairs: Option<&TokenPairs>)`; `event_token_pairs(stored, keys, account_id)` opens only the stored envelope and bundle | `seal/event.rs` |

## Rulings

- `sync-collection` after a drain (Task 7 Step 3):
  - While the item is pending, a sync does not report it at all, but the token still advances past its insert.
  - After the drained rewrite (same document id, pending bit cleared), the item is reported exactly once, as an ordinary member with a 200 propstat. That holds both from the token taken while it was pending and from the token taken before it arrived, and the two responses are byte-identical.
  - On the wire, a drained item looks like a newly added member (RFC 6578). It is never missing and never reported twice, and a GET of it then answers 200.
- Inbox expiry:
  - Reached through `Task::AccountMaintenance` (type `Purge`), created through the registry and run by the task manager with no session of the account.
  - Default hold: 30 days, the code default of `expungeSchedulingInboxAfter`, 2592000000 ms (`crates/registry/src/schema/structs_impl.rs:6604`). A pending item created 31 days ago was removed, and one created now was kept.
  - No fallback was taken: neither `StoreMaintenance`/`PurgeAccounts` nor a `DataRetention` edit.
- `assert_is_empty`: za sub-modules do not call it; they clean up, and the suite end checks (plan Global Constraints).
- `hkdf_expand` takes the full HKDF info, so the `pk` derivation appends `E || P` to the label like `derive_app_kek`; no sibling function.
- Format 2 bundles bind their format byte into the associated data.
- Inbox item envelopes and bundles bind the recipient and the UID, in release 1's event-key shape (spec amendment `971008e1`, sections 3.1, 5.3 and the section 9 table). The associated data:
  - pending key envelope: `za/v1|pk-dek|<recipient>|<uid>`;
  - pending item's `X-ZA-EXTRA` bundle: `za/v1|itip-extra|<recipient>|<uid>`;
  - drained key envelope: `za/v1|itip-key|<owner>|<uid>`.

  `<recipient>` and `<owner>` are `aad`'s 4-byte big-endian account id. The amendment also dropped the doubled `za/v1` prefix: the purpose string is `pk-dek`.
- Branch: implementation ran on `release2-invitations-plan`, not a new `plan8-*` branch as the plan's Global Constraints suggest. The branch was cut from main for this plan and already carried the plan and the spec amendment.
- The fork's test-harness note in CLAUDE.md (where `assert_is_empty` runs in each suite) was corrected by the controller in `dabae7af`, not by a task. It is a one-line fact, verified by grep.
- Commits carry two trailers, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr`. Dispatches stated them verbatim, and a wrong trailer is rewritten before the branch is shared.
- Task 5:
  - The plan's `test.wait_for_tasks()` before the DELETE loop in `tests/src/za/pairs.rs:182` never returned. The alarm-only PUT queues a `CalendarAlarmNotification` task due in 2099.
  - Ruling: the line became `test.wait_for_tasks_skip_not_due()`. It still drains the due index tasks before the DELETEs, which is why the wait sits there (PR #30's orphaned-index lesson).
  - The suite-end full wait proves that the DELETE cleared the 2099 task.
- Task 6:
  - GET and the PROPFIND loader check `za_item_hidden` before `za_session_keys` (brief Step 4 order). A keyless member session of a key account (in practice the master user) therefore sees 404 for a pending item and 403 for any other item. That discloses the plaintext pending bit per href.
  - Ruling: the design stands. The bit is plaintext metadata the operator already reads from the store, and release 1 treats it as such. Swapping the order would add a `Security(Unauthorized)` trace per such request. The final review may still weigh it.
- Task 9: every mutation failed as predicted, so no (a)/(b) decision was needed and no test was added.

## Mutation runs (plan 8)

Line numbers are those of the committed code at `eca18e12`. Each edit was reverted with `git checkout -- <file>`, and `git status --short crates/` printed nothing before the next one.

| Id | Check, `file:line` | Edit | First failure | Command |
|----|--------------------|------|---------------|---------|
| M16 | Pending skip in the PROPFIND loader, `crates/dav/src/common/propfind.rs:463` | delete the `if za_item_hidden(..) { continue; }` block after `stored_ = archive;` | `tests/src/za/inbox.rs:130:5`: `pending item listed: [".../inbox/", ".../inbox/1.ics", ".../inbox/2.ics", ".../inbox/3.ics"]` | za suite |
| M17 | Pending 404 on GET/HEAD, `crates/dav/src/calendar/scheduling.rs:119` | delete the `if za_item_hidden(..) { return Err(..NOT_FOUND) }` block | `tests/src/utils/webdav.rs:599:13`: `Expected 404 Not Found but got 500 Internal Server Error`, from the pending GET at `tests/src/za/inbox.rs:149-152`, the first request after the listing | za suite |
| M18 | Pairs at the update seal site, `crates/dav/src/calendar/update.rs:337` | `seal_event(&mut new_event, keys, account_id, None)` in the update branch | `tests/src/za/pairs.rs:51:41`: `no token pairs after an alarm-only PUT` (`assert_both`, called at `pairs.rs:100`) | za suite |
| M19 | Pairs at the PROPPATCH seal site, `crates/dav/src/calendar/proppatch.rs:259` | `None` instead of `pairs.as_ref()` | `tests/src/za/pairs.rs:51:41`: `no token pairs after a PROPPATCH` (called at `pairs.rs:120`, after the alarm-only PUT step passed) | za suite |
| M20 | Carrier order on pending items, `crates/groupware/src/calendar/seal/item.rs:199` (`open_pending_item`) | delete the `has_stray_carriers` check | `pending_item_carriers_out_of_order_are_stray`, at `item.rs:458:13` in the mutated file (`:461` committed): `assertion left == right failed: swapped`, `left: None`, `right: Some(Structure("misplaced carrier"))`; 9 passed, 1 failed | `cargo test -p groupware seal::item` |
| M21 | The inbox view arm, `crates/dav/src/common/za.rs:224` (`za_archive_view`) | delete the `Collection::CalendarEventNotification` arm | `tests/src/za/inbox.rs:139:5`, the listing body assertion. The dumped body shows `2.ics` (drained) as its sealed tree, `X-ZA-KEY` and `X-ZA-SEALED` with no SUMMARY, and `inbox-drained-canary` appears nowhere in the log | za suite |

## Facts for plans 9 and 10

- Plan 9 calls:
  - `seal_pending_item(ical, recipient_id, &public_key, pair, reply_token)`: one pair or one token, never both; a low-order public key is `Structure("recipient public key")`.
  - `TokenPairs::{insert, get, only, matches}`.
  - `event_token_pairs` and `seal_event(.., Some(&pairs))`.
  - `EVENT_NOTIFICATION_ZA_PENDING`.
  - The create branch's `None` in `crates/dav/src/calendar/update.rs` becomes the organizer's pairs.
- Plan 10 calls:
  - `Server::za_private_key(&keys)`, once per pass.
    - A recorded failure (the wrap does not open, the record is missing or unusable) was traced once for the session and is an error on every call.
    - A propagated store error was not traced, and the drain must trace it (Task 4 review). It is also not recorded, so a later call retries.
    - The doc comment at `crates/common/src/auth/vault.rs:464-465` still says "an error was already traced for the session"; that holds only for a recorded failure.
  - `open_pending_item` (returns `PendingCarriers`), `seal_drained_item`, `archived_item_is_pending`, `TokenPairs::matches`.
  - `rewrite_inbox_item` and `plant_inbox_item` (`tests/src/utils/za.rs`) for its tests.
- `open_pending_item` returns whatever pair count the bundle holds; only the seal side enforces one pair. Anyone holding the recipient's public key can seal a pending item. The drain must therefore check `pair.len() == 1`, unless the final review's fix wave adds the check to the opener (Task 3 review).
- DELETE of a pending item by its guessable href is upstream's DELETE, unchanged; spec 6.6 hides items from listings, reports and GET only.
- A pending item's envelope and bundle bind the UID, so an item whose sealed parts were moved onto another UID's record does not open; the drain treats that as a permanent open failure (spec 6.5). Component bundles bind the account, the `itip` scope and the component index.
- What the leak test's `token_hits` covers (`tests/src/za/leak.rs`), from the Task 8 review:
  - It searches three forms of each token: raw bytes, base64url without padding and padded standard base64.
  - It searches keys, values, decoded archives and decoded blobs.
  - It does not decode MIME bodies, so plan 10's mail flows need that added.
  - It skips blobs it cannot read.
- The views judge an item pending by its envelope's wrap type, not by `EVENT_NOTIFICATION_ZA_PENDING`. An item with the flag set and an `mk` envelope would open in the view; hiding by the flag is `za_item_hidden`'s job (Task 3).
- `tests/src/za/leak.rs:686` ("the alarm email was not delivered") flaked once in Task 5 (0 emails) and passed on an unchanged rerun. It counts emails after a fixed 5-second sleep (`leak.rs:612-614`) behind a 1-second alarm. It did not flake in Task 9.

## Deferred findings

None of these was fixed during execution. The final branch review comes after this note and decides the fix wave. Line references are as the reviews gave them, at the reviewed commit.

Task 1 (`pk` wrap):
- Tamper and truncation are asserted with `is_err()` only (`keys.rs:485-486`). Reason: plan-mandated text.
- Only the all-zero low-order key is tested (`keys.rs:511-519`). Reason: minor test breadth.
- The `[u8; 32]` stack copies at `keys.rs:267`/`:285` are not zeroed, same as `generate_keypair`. Reason: existing practice.
- Spec 3.1 step 4 (`spec:147`) and spec 13 (`spec:1048`) still write `aad("za/v1/pk-dek", r)` / `za/v1/pk-dek`. The constant is `"pk-dek"` and the caller appends `"|" || uid`. Reason: a one-line doc touch-up with the final docs.

Task 2 (pairs, format 2):
- Heap residue when a `Zeroizing<Vec>` grows (the push in `TokenPairs::insert`, `rkyv::to_bytes` in `seal_extra`). Reason: inherent to `Vec`; the vault accepts the same class.
- `format_aad`'s `|\x02` suffix follows the unescaped UID field (`tree.rs:177-187`). Reason: not exploitable (per-write DEK, UID-bound envelope); a comment would help.
- No golden release 1 ciphertext vector pins format 1 compatibility. Reason: out of the brief's scope.

Task 3 (sealed items):
- `open_pending_item` does not re-check that the opened bundle holds exactly one pair (`item.rs:202-212`). Reason: not in the brief; a 3-line check plus a test, for the final review to weigh for the fix wave.
- `pair.and_then(seal_extra)` would silently drop a pair if the one-pair guard were relaxed (`item.rs:160-170`). Reason: the guard holds today.
- Untested but guaranteed by the code:
  - `open_pending_item` on an `mk` item gives `Format`;
  - release 1 openers refuse a `pk` envelope (`open_key_envelope` gives `Format`);
  - the order `[.., SEALED, EXTRA, RSVP, KEY]` is rejected.

  Reason: minor test breadth.
- The views judge pending by wrap type. Reason: by design, and correct.
- 19 pre-existing upstream warnings in the build output (`store` 1, `common` 18). Reason: upstream's.

Task 4 (session private key):
- The doc on `Server::za_private_key` says "an error was already traced for the session" (`vault.rs:462-463` in the review; the comment's lines are `:464-465`). That is true only for a recorded failure: a propagated store error is untraced, and plan 10's drain must trace it. Reason: plan-mandated wording; carried into the final fix wave and into the facts above.
- The missing- and moved-record tests assert `is_err()` only (`vault.rs:941-956`). The store-error-not-recorded row has no test. Reason: `EphemeralStore` cannot fail.
- The unusable-record arm traces the generic message, losing the corrupt-vs-bad-wrap distinction. Reason: conforms to the brief.
- The plan's warning filter `crates/common/src/auth/` matches four pre-existing upstream warnings (`permissions.rs:48`; `authentication.rs:661`, `:698`, `:720`). Reason: the filter is too broad; this is not a defect.

Task 5 (pairs at both rewrite sites):
- No PUT follows the PROPPATCH on the paired event, so a display name and pairs together through the update site is untested (`pairs.rs:114-127`). Reason: a Task 2 unit test covers the V2 arm's `display_name` round trip.
- The stored archive is opened twice per rewrite (the view, then `event_token_pairs`). Reason: brief design.
- The comment label "Review Focus 5" at `pairs.rs:129` should become a spec reference. Reason: cosmetic.
- The fixed 5-second sleep at `leak.rs:612-614` is the likely cause of the `leak.rs:686` "alarm email was not delivered" flake. Fix: replace it with `wait_for_delivery`, or document the test as flaky. Reason: pre-existing.

Task 6 (inbox read paths):
- A token-bearing `sync-collection` across the rewrite is untested in Task 6. Reason: Task 7's `test_sync` covers exactly that.
- The key-account status is checked and the archive decoded twice per item. Reason: brief shape.
- `parse_ical` is duplicated in `inbox.rs` and `utils/za.rs`. Reason: both are brief-mandated.

Task 7 (sync and expiry):
- `test_sync`'s 200 check scans the whole body (`inbox.rs:331-332`). Fix: add `response.hrefs().len() == 1`. Reason: plan-mandated text; a cheap tightening for the final fix wave.
- The pre-drain check counts only the planted href (`inbox.rs:311`). Fix: add `hrefs().is_empty()`. Reason: as above.
- No `assert_ne!(t0, t1)` pins that the token advances past a skipped insert (`inbox.rs:312`). Reason: as above.
- The `__eh_frame section too large` linker note. Reason: macOS large-binary noise, not a code warning.

Task 8 (leak coverage). The ledger marks the findings on `token_hits` skipping blobs, the missing `STANDARD_NO_PAD` form, the reply-record exemption, the vacuous stray assert and the canary substring as cheap hardening for the final fix wave; the rest are plan 10 notes.
- `check_item` never checks the popped carriers for parameters or the RSVP value shape (`leak.rs:298-307`).
- `token_hits` silently skips unreadable blobs (`if let Ok(Some(..))`, `:624`), where `scan` records a violation.
- There is no `STANDARD_NO_PAD` form, so an unpadded standard encoding (with `+`/`/`) of a random token would be missed (`:565-567`).
- `token_hits` does not decode MIME bodies as `scan` does. Reason: brief scope; it matters for plan 10's mail flows.
- The reply-record exemption covers any form of token 1 on that record, not only the url-safe one (`:942-943`).
- Nothing asserts that the request item and the paired event actually carry `X-ZA-EXTRA` (`:851-893`), so the stray assert could pass vacuously for tokens 0, 2 and 3.
- `token_hits` duplicates `scan`'s store walk (`:579-620`).
- A doc comment says "key2's pair", but the pair address is the canary (`:98`).
- `negative-canary` is a substring of `itip-negative-canary`, and the event control's asserts are not key-filtered (`:73` vs `:1016`/`:1023`).
- Token index 1 is hard-coded (`:943`).
- `check_item` accepts `X-ZA-RSVP-TOKEN` regardless of flags or METHOD.
- The `__eh_frame` linker note (as in Task 7).
