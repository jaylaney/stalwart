# Zero-access plan 12: key pinning (server side) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A key account's fan-out compares each contact's served public key with the key it pinned for that address and refuses to seal to a different one (`3.8`), an account repairs a substituted copy of its own public key, and four password-verified endpoints under `/api/vault/contacts` list, look up, confirm (redelivering what was withheld) and forget pins.

**Architecture:**
- **Primitives** (Task 1, `crates/vault`): the pin record of spec 4.1 sealed under MK with `aad("pins", id)`, the display fingerprint (3.1), and the two private-key wrap purposes, `private-key` (legacy) and `private-key/pins` (pinned, 4.4). `PrincipalField::ZeroAccessPins = 151`.
- **Storage** (Tasks 2-3, `crates/common/src/auth/vault.rs`): one commit helper for vault writes (`za_vault_commit`, which `za_commit` wraps, 4.5); the conditional pin-record write with eight attempts, which becomes the first-pin or heal batch under a legacy wrap (4.2, 4.4); the public-key-only read of a recipient's served key (5.1); the self-check (3.3), run on every session's first drain pass and by every contacts endpoint.
- **Fan-out** (Tasks 4, 6, 7, `crates/groupware/src/calendar/{fanout,sequence,pinning}.rs`): `za_fan_out` takes the source `(document_id, uid)` and a kind (`Change`, `Deletion`, `Redelivery`) and runs in the four phases of 5.2. Before phase 1 the SEQUENCE reconciliation (5.1) makes the committed tree and every organizer message agree per instance; the PUT handler's SEQUENCE floor keeps a client from writing lower values back. A delivery to an address with a withheld entry writes the full current state first (the two-item rule).
- **Endpoints** (Task 5, `crates/http/src/api/contacts.rs`): `contacts`, `contacts/lookup`, `contacts/pin` (pin, drain, redeliver up to 32 entries, guarded cleanup) and `contacts/forget` (section 6).

The fork diff stays narrow: new modules (`vault::pins`, `groupware::calendar::{pinning, sequence}`, `http::api::contacts`, `tests/src/za/pinning.rs`) and edits in the fork's own modules (`fanout.rs`, `drain.rs`, `common/src/auth/vault.rs`, `http/src/api/vault.rs`, `test_hooks.rs`). Upstream files change only at fork call sites: `crates/types/src/field.rs` (one variant), `crates/dav/src/calendar/update.rs` (the floor call, the create's document id before the fan-out, fan-out arguments), `delete.rs` (fan-out arguments), `crates/groupware/src/calendar/{storage,mod}.rs`, `crates/http/src/api/mod.rs` (one `mod` line). Upstream's iTIP builders are not edited.

**Tech Stack:** Rust 2024 (Stalwart 0.16.26 fork; the `vault` crate is 0.16.25); x25519-dalek 2, chacha20poly1305 0.11, sha2 0.11, rkyv 0.8.18 (checked decode), calcard 0.3, xxhash-rust (`xxh3_64`, the store's `AssertValue::Hash`); the `tests` crate harness (`TestServer`, `DummyWebDavClient`, `HookParked`, `DrainOn`).

**Spec:** `docs/superpowers/specs/2026-10-17-zero-access-key-pinning-design.md`, revision 11 (approved), cited by section number. Release 1 (`2026-10-06-zero-access-calendar-design.md`) and release 2 (`2026-10-11-zero-access-invitations-design.md`) bind where the spec cites them; "api.md" is `/Users/jay/Development/circulo-account/docs/api.md`. Project rules: `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, the plan 3 outcome note. The plan 11 outcome note's deferred findings stay open; none is resolved here.

**Spec 13, the open items, as this plan resolves them:**
1. **Trace event: confirmed.** `Security(SecurityEvent::Unauthorized)` (level Info) is what the drain's `drop_forgery` traces for a forged reply token (`crates/groupware/src/calendar/drain.rs:1285-1291`). The self-check mismatch and the refusal use it with fixed `zero-access:` details (Global Constraints).
2. **Field 151: confirmed unassigned.** The repository carries upstream's whole history (not shallow; `git rev-list --all --count` is 2109, from 2023-03-06). `git log --all -G '= *151([^0-9]|$)' -- '*.rs'` finds four commits (`0398a9d2`, `9f9ddee9`, `95d12683`, `776bf7cb`), all in other key spaces: `crates/registry/src/schema/{enums,properties}.rs` (`Property::Accounts = 151` and the Locale, Permission and TimeZone enums) and `crates/trc/src/event/enums.rs` (event ids). The match-arm forms, `git log --all -G '151 *=>|=> *151' -- '*.rs'`, find the same four (their `*_impl.rs` mappings) plus `abd318b6` (`crates/trc/src/serializers/binary.rs`, `HousekeeperEvent::Stop => 151`, an event id) and `07449a36` (`crates/nlp/src/tokenizers/japanese.rs`, a weight table whose values merely begin with 151). None is a `PrincipalField`. The pre-`types` `jmap-proto` `Property` mapping that keyed Principal properties before 0.13.4 tops out at 103 and never casts with `as u8`. 151 it is (Task 1).
3. **The REPLY helper's pieces: no upstream edit.** A fork module in `groupware` reaches everything: `itip_snapshot` (pub), `itip_build_envelope`, `itip_export_component` with `ItipExportAs::Attendee`, `itip_add_tz` (all `pub(crate)`, `scheduling/itip.rs`), `ItipSnapshot::{local_attendee, external_attendees}` and `Attendee::{is_delegated_from, is_delegated_to}` (pub). The helper (Task 5) mirrors `attendee_handle_update`'s per-instance export (`attendee.rs:111-153`) without its "changed" predicate. The fork DECLINED of 5.1 item (2) (Task 7) is a fork copy of `attendee_decline` (`attendee.rs:279-364`) without the PARTSTAT match at lines 301-308.
4. **SEQUENCE shadowing: confirmed; the post-pass is the plan's choice under spec 13's preamble (the recommendation stands); Jay can override it at plan review.** The partial branch of `organizer_handle_update` declares `let mut increment_sequences = Vec::new();` (`organizer.rs:234`), shadowing the parameter (line 27), and its CANCEL arm (lines 266-268) records nothing. Task 6 implements section 5.1's fork post-pass and leaves upstream's builders untouched. The traced panic (the tree switch at lines 236-245 is permanent; a removed override looked up in the new tree after a new STATUS:CANCELLED override sorted first, `unwrap` at line 243) is checked by a throwaway probe in Task 6, whose result goes in the outcome note; nothing is fixed or reported upstream.
5. **Organizer-side delegate guard: deferred**, outside this plan; Task 8 records it in the outcome note with plan 11's deferred item.
6. **Client display of `3.8`:** manual checklist item 15, no code (Task 8).

**Execution:** subagent-driven development on a new branch cut from `key-pinning-spec` (the spec at `eb219e60`, this plan committed on top); implementers run at `opus`, reviewers at the session's model ceiling (`opus` today); ledger `.superpowers/sdd/2026-10-17-zero-access-12-key-pinning/progress.md`.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build with `enterprise`.
- Per-task compile check, shipping configuration; the build must succeed (exit 0) before the warning filter runs, so a failed build is never read as `clean`:
  ```bash
  log="${TMPDIR:-/tmp}/za-check.log"
  cargo check -p stalwart --no-default-features --features rocks > "$log" 2>&1 || { tail -40 "$log"; echo BUILD FAILED; false; }
  grep -E "^(warning|error)" -A3 "$log" | grep -E "crates/(groupware|dav|vault|types)/|crates/http/src/(request.rs|api/)|crates/common/src/(za/|auth/vault.rs)" || echo clean
  ```
  Upstream's known warnings in store, common, jmap and services are not this plan's.
- Test-mode compile check: `cargo check -p tests --tests`.
- Unit tests: `cargo test -p vault`, `cargo test -p groupware`, `cargo test -p common auth::vault`, `cargo test -p common za::`.
- Integration: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`; at the end also with `BLOB_STORE=FileSystem`. CalDAV: the same prefix with `webdav_tests`, plain and with `ZA_KEY_ACCOUNTS=1`; upstream's `cal_itip` sub-test is timing-flaky, rerun once before calling a failure there a regression.
- One cargo command at a time; never while Jay's manual server (ports 8080/8443/1143) is under test.
- The formatting check (CLAUDE.md's forms): `cargo fmt -p <crate> -- --check` for each touched crate other than http (`vault`, `types`, `common`, `groupware`, `dav`, `tests`), and `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` for http. Build output warning-free for new code.

Spec values, verbatim:
- `PrincipalField::ZeroAccessPins = 151`, keyed like 150 (account id, `Collection::Principal`, document 0).
- Associated data purposes: `"pins"` (`AAD_PINS`), `"private-key/pins"` (`AAD_PRIVATE_KEY_PINNED`); the legacy `"private-key"` (`AAD_PRIVATE_KEY`) is unchanged.
- Stored pin record: `nonce (24) || XChaCha20-Poly1305(u32 big-endian length || rkyv ZaPinRecord || zero padding to a multiple of 256) || tag (16)`; nothing precedes the nonce; `version` `1`, any other value unusable (4.1, 7).
- `ITIP_STATUS_KEY_REFUSED = "3.8"`, rank 3 above `5.1` (2) and `5.3` (1) (5.3).
- Eight attempts per pin-record write (`ZA_PIN_WRITE_ATTEMPTS = 8`); the eighth loss is `5.1` in a fan-out and 409 `{"error": "conflict"}` from an endpoint (4.2).
- 64 instance keys per entry (`INSTANCE_KEY_CAP`), the master not counted; 32 entries per `contacts/pin` call (`REDELIVER_BATCH`).
- Fingerprint: the first 16 bytes of SHA-256 over the 32-byte key, lowercase hex, 8 space-separated groups of 4 (3.1).
- Endpoint 409 strings: `not a key account`, `fingerprint mismatch`, `conflict`, `vault record missing`; success bodies as section 6 gives them.
- All of these are constants: no setting, permission, event type or environment knob (1.4); no edit of generated code (`crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`).

Invariants and code rules:
- Release 1's invariants 1-10, release 2's 11-15 and spec 10's 16-23 hold. In short: the pin record is sealed under MK and touched only in a request holding the owner's session keys, never by background code (16); pins are compared before any item, phase 2's write precedes the items and phase 4's follows them, and a failed retirement undoes nothing (17); pins are keyed by the normalized address and compared with the key read fresh from the recipient's vault record (18); the own fingerprint and the self-check derive the public key from the private key (19); the pinned wrap never reverts and the vault record's layout does not change (20); no key bytes, fingerprints or addresses in traces (21); under a pinned wrap only `contacts/pin` and `contacts/forget` with null create a record, under a legacy wrap the fan-out or `contacts/pin` creates it atomically with the rewrap, the self-check and the listing never do (22); entries are bound to the source document, hold no message, and are retired only under their consumed `refusal_id` (23).
- No stored struct changes layout: field 151 is new; `VaultRecord` and `private_key_wrap` (72 bytes) keep theirs.
- Non-key accounts take unchanged paths (invariant 9): pins, the floor and the reconciliation run only for key accounts. Upstream's builders and merge run unchanged (invariant 15).
- Traces carry account ids, document ids, counts and fixed strings only. The self-check mismatch: `Security(SecurityEvent::Unauthorized)`, `AccountId`, `Details = "zero-access: stored public key does not match the private key"`. A fan-out's refusals: the same event, once per fan-out, `AccountId` (the sender), `Total` (refused recipients), `Details = "zero-access: delivery refused, the served key differs from the pin"`. Failures: `trc::error!` with the sender's id and `zero-access: pin record read failed`, `zero-access: pin record write failed`, `zero-access: withheld entries not retired`, `zero-access: self-check failed`.
- Key material and decrypted plaintext live in `Zeroizing` buffers; `ZaPinRecord`, `ZaPin` and `ZaWithheld` have a `Debug` that prints counts only (they hold addresses).
- Every vault write in product code goes through `Server::za_vault_commit` or `za_commit`; never `za_vault_write` directly (Task 2 writes the rule into `CLAUDE.md`). Test helpers act as the store writer and may write raw.
- Test addresses: `key1@example.com` (alice), `key2@example.com` (bob), `key3@example.com` (carol), `plain@example.com`, sub-addresses of these, and `.test` for any new domain; never `.example`.
- Commit messages: subject, blank line, then exactly `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr`.
- Never open issues, pull requests or reports against upstream `stalwartlabs/stalwart`; upstream bugs found here go in the outcome note only.

Tests:
- The za suite skips every drain (`DRAIN_SKIP_ALL`); `pinning::test` holds `DrainOn` for its duration. It runs after `drain_limits::test` and before the leak tests.
- Each pinning case is one `async fn case_NN_<name>(test: &TestServer)`, called from `pinning::test` in case-number order with the `rf_*` tests last, self-contained: it puts the accounts into the state it names (`za_unpin`, `substitute_public_key`), restores every substituted key with cache invalidation before it returns, deletes its events with `Schedule-Reply: F` (`NO_REPLY`) and its inbox items by href, and leaves no account fail-closed (`contacts/forget` with null or `za_unpin`) and no withheld entry (where a refusal remains, Task 5's `drop_entries` after deleting its events). Pin records and pinned wraps may remain: `assert_is_empty` exempts fields 150 and 151 while the account exists.
- Every case in which bob's REPLY is refused first gives bob a record pinning alice's real key, `write_pin_record(test, bob.id, STRONG, &record_pinning(ALICE, served(test, alice.id).await))`, as case 3 does, so the refusal does not depend on earlier cases; such setup lines say "bob pins alice first".
- A substituted key of an account that makes a new-session DAV request is repaired by that account's own self-check (3.3). A case that substitutes X's key makes no DAV request as X until it restores it, unless the repair is what it tests.
- Every PUT of an existing event is built from a GET of the stored event, with SEQUENCE set relative to the stored value where a case says "the client bumps" or "lower" (spec 9). "Item order" is the recipient's inbox in document-id order.
- New hooks are cleared at the end of the step that set them. `wait_for_delivery` is never called after a fan-out (it is synchronous).
- An existing za or webdav assertion that fails only because a fan-out now pins (the first pin's generation bump, a sender's new pin record) is adjusted with a comment citing spec 4.4 and listed in the task report; any other failure is a regression.
- Kill checks and mutation runs are made against a commit, one at a time, each reverted with `git checkout -- <file>` and followed by an empty `git status --short crates/`; none is committed.

## Review Focus

Inputs the spec implies that no case of section 9 exercises, most likely to bite first. Each has its test in the owning task.

1. **Address forms.** An ATTENDEE written `mailto:Key2@Example.COM` and the page's ` Key2@Example.COM ` must reach one pin keyed `key2@example.com`; `contacts/lookup` with `mailto:key2@example.com` or `Key2 <key2@example.com>` is 400, not a second pin (4.3). Task 5, `rf_1_address_forms`.
2. **Mixed recipients in one fan-out.** With bob's key substituted and carol's not, one edit refuses only bob (`3.8`, one entry) and delivers to carol (`1.2`, an item): a refusal is per recipient, never the whole fan-out. Task 4, `rf_2_mixed_recipients`.
3. **Two sessions of one sender first-using different contacts at once.** The loser of the phase 2 write re-reads and re-decides; both pins exist, both items are written, nobody gets `5.1`. Task 4, `rf_3_concurrent_first_use`.
4. **A withheld source moved to another calendar.** A MOVE keeps the document id (4.1), so `contacts/pin` redelivers from the moved event instead of dropping it. Task 5, `rf_4_moved_source`.
5. **An account that never pinned opens the contacts page.** The listing answers `record_missing: false`, `pins: []`, `key_repaired_at: null`, creates no record and leaves the wrap legacy (invariant 22). Task 5, `rf_5_never_pinned`.

## Case map

| Spec 9 item | Task |
|---|---|
| Unit: fingerprint, pin record, wraps, `refusal_id`, recorded-instance union and cap | 1 |
| Unit: conditional pin writes, the batch, the mark, the served key, normalization | 2 |
| Case 3 | 3 |
| Cases 1, 9, 16; unit: `status_rank`, `3.8` replaced, instances by instant | 4 |
| Cases 2, 4, 6, 7, 8, 10, 14, 15, 17, 18, 19, 20; unit: the REPLY helper | 5 |
| Case 12; unit: reconciliation, floor | 6 |
| Cases 5, 11, 13, 21; unit: the fork DECLINED | 7 |
| Leak scan, key-mode `webdav_tests`, every mutation | 8 |

---

### Task 1: Key-trust primitives

**Files:**
- Modify: `crates/vault/src/keys.rs` (constants after `PUBLIC_KEY_LEN`, line 162; functions after `generate_keypair`, line 237; tests in `mod tests`, line 302)
- Create: `crates/vault/src/pins.rs`
- Modify: `crates/vault/src/lib.rs` (`pub mod pins;`)
- Modify: `crates/types/src/field.rs:78-90` (variant), `:161-172` (`From<PrincipalField> for u8` arm)

**Interfaces:**
- Consumes: `keys::{seal, open, aad, wrap_key, unwrap_key, Secret, KeyError, PUBLIC_KEY_LEN, NONCE_LEN, TAG_LEN, WRAP_LEN, AAD_PRIVATE_KEY}`.
- Produces, in `vault::keys`:
  - `pub const AAD_PINS: &str = "pins";`, `pub const AAD_PRIVATE_KEY_PINNED: &str = "private-key/pins";`, `pub const FINGERPRINT_LEN: usize = 16;`
  - `pub fn fingerprint_bytes(public_key: &[u8; PUBLIC_KEY_LEN]) -> [u8; FINGERPRINT_LEN]` (SHA-256 prefix); `pub fn display_fingerprint(public_key: &[u8; PUBLIC_KEY_LEN]) -> String`; `pub fn parse_fingerprint(text: &str) -> Option<[u8; FINGERPRINT_LEN]>` (spaces removed, either case, exactly 32 hex digits, as `contacts/pin` accepts it, 6.4).
  - `pub fn derive_public_key(private_key: &Secret) -> [u8; PUBLIC_KEY_LEN]` (`x25519_dalek::PublicKey::from(&StaticSecret)`, 3.2).
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum WrapMark { Pinned, Legacy }`; `pub fn wrap_private_key(private_key: &Secret, mk: &Secret, account_id: u32, mark: WrapMark) -> Vec<u8>`; `pub fn unwrap_private_key(wrap: &[u8], mk: &Secret, account_id: u32) -> Result<(Secret, WrapMark), KeyError>` (pinned first, then legacy; `Err(KeyError::Aead)` when neither opens, 4.4).
- Produces, in `vault::pins`:
  - `pub const PIN_RECORD_VERSION: u8 = 1;`, `pub const INSTANCE_KEY_CAP: usize = 64;`, `pub const PIN_PAD: usize = 256;`
  - The structs of spec 4.1 with its fields, types and order: `ZaPinRecord { version, revision, pins, withheld, key_repaired_at }`, `ZaPin { address, public_key: [u8; 32], pinned_at, source }`, `PinSource { FirstUse, Confirmed }`, `ZaWithheld { document_id, uid, address, since, refusal_id, instances }`, `ZaInstances { master: bool, keys: Vec<(i64, bool)>, all: bool }`. Derives: `rkyv::{Archive, Serialize, Deserialize}`, `Clone`, `PartialEq`, `Eq`; `Copy` on `PinSource`; `Default` on `ZaInstances`; `Debug` derived on `PinSource` and `ZaInstances`, hand-written on the other three (counts only).
  - `pub fn seal_pin_record(record: &ZaPinRecord, mk: &Secret, account_id: u32) -> Vec<u8>`; `pub fn open_pin_record(stored: &[u8], mk: &Secret, account_id: u32) -> Option<ZaPinRecord>` (`None` is "unusable": does not open, does not decode, or `version != 1`). The decode is checked (`rkyv::from_bytes` over an aligned copy zeroed afterwards, as `decode_archive` in `crates/groupware/src/calendar/seal/tree.rs:287-303` does); the length prefix is big endian (spec 7).
  - `pub fn new_refusal_id() -> u64` (64 random bits).
  - `impl ZaPinRecord`: `pub fn new() -> Self` (version 1, revision 0, empty, `key_repaired_at: None`); `pub fn pin(&self, address: &str) -> Option<&ZaPin>`; `pub fn set_pin(&mut self, address: &str, public_key: [u8; 32], source: PinSource, now: i64)` (replaces any pin for the address); `pub fn remove_pin(&mut self, address: &str) -> bool`; `pub fn entry(&self, document_id: u32, uid: &str, address: &str) -> Option<&ZaWithheld>`; `pub fn entries_of<'a>(&'a self, document_id: u32, uid: &'a str) -> impl Iterator<Item = &'a ZaWithheld>`; `pub fn refuse(&mut self, document_id: u32, uid: &str, address: &str, now: i64, instances: Option<&ZaInstances>) -> u64` (creates the entry with `since = now` or re-arms the existing one; a fresh `refusal_id` either way, returned; `instances` unioned in); `pub fn retire(&mut self, document_id: u32, uid: &str, address: &str, consumed: u64) -> bool` (removes the entry only when its `refusal_id == consumed`). Writers bump `revision` (Task 2); these methods do not.
  - `impl ZaInstances`: `pub fn add(&mut self, key: Option<(i64, bool)>)` (`None` is the master; adding a 65th distinct key sets `all` and clears `keys`, which stay empty after); `pub fn union(&mut self, other: &ZaInstances)`; `pub fn contains(&self, key: Option<(i64, bool)>) -> bool` (`contains(None) == master || all`; an override key is contained when `all` or listed in `keys`: under `all` every qualifying instance, the master included, 4.1).
- Produces, in `types::field`: `PrincipalField::ZeroAccessPins = 151`, with a doc comment in the shape of `ZeroAccessVault`'s: never assigned to any field in upstream's history (open item 2), so leftover bytes cannot read as a record.

- [ ] **Step 1: Write the failing unit tests in `keys.rs`**

```rust
/// RFC 7748 section 6.1, Alice's public key.
const RFC7748_ALICE: &str = "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";

#[test]
fn display_fingerprint_is_the_sha256_prefix_in_eight_groups() {
    let key: [u8; 32] = hex32(RFC7748_ALICE); // a local test helper
    assert_eq!(display_fingerprint(&key), "300c 9c96 03b9 2a4b 39ed 3958 bf92 4011");
    assert_eq!(display_fingerprint(&[0u8; 32]), "6668 7aad f862 bd77 6c8f c18b 8e9f 8e20");
    assert_eq!(parse_fingerprint("300C9C9603B92A4B 39ED 3958 BF92 4011"), Some(fingerprint_bytes(&key)));
    for bad in ["", "300c 9c96", "300c 9c96 03b9 2a4b 39ed 3958 bf92 40110", "zz0c 9c96 03b9 2a4b 39ed 3958 bf92 4011"] {
        assert_eq!(parse_fingerprint(bad), None, "{bad}");
    }
}

#[test]
fn pinned_and_legacy_wraps_open_only_under_their_own_purpose() {
    let mk = Secret::random();
    let (public, private) = generate_keypair();
    let pinned = wrap_private_key(&private, &mk, 7, WrapMark::Pinned);
    let legacy = wrap_private_key(&private, &mk, 7, WrapMark::Legacy);
    assert_eq!((pinned.len(), legacy.len()), (WRAP_LEN, WRAP_LEN));
    assert!(unwrap_key(&pinned, &mk, &aad(AAD_PRIVATE_KEY, 7)).is_err());
    assert!(unwrap_key(&legacy, &mk, &aad(AAD_PRIVATE_KEY_PINNED, 7)).is_err());
    let (key, mark) = unwrap_private_key(&pinned, &mk, 7).unwrap();
    assert_eq!((mark, derive_public_key(&key)), (WrapMark::Pinned, public));
    assert_eq!(unwrap_private_key(&legacy, &mk, 7).unwrap().1, WrapMark::Legacy);
    assert_eq!(unwrap_private_key(&pinned, &mk, 8).err(), Some(KeyError::Aead));
}
```

- [ ] **Step 2: Write the failing unit tests in `pins.rs`**

```rust
#[test]
fn a_pin_record_round_trips_and_binds_account_and_mk() {
    let (mk, record) = (Secret::random(), sample_record()); // two pins, one entry with instances
    let sealed = seal_pin_record(&record, &mk, 7);
    assert_eq!(open_pin_record(&sealed, &mk, 7), Some(record.clone()));
    assert_eq!(open_pin_record(&sealed, &mk, 8), None, "another account's associated data");
    assert_eq!(open_pin_record(&sealed, &Secret::random(), 7), None, "another MK");
    assert_eq!(open_pin_record(&sealed[..sealed.len() - 1], &mk, 7), None);
    assert_eq!(open_pin_record(b"garbage", &mk, 7), None);
}

#[test]
fn the_plaintext_pads_to_a_multiple_of_256_bytes() {
    let mk = Secret::random();
    assert_eq!(seal_pin_record(&ZaPinRecord::new(), &mk, 7).len(), NONCE_LEN + 256 + TAG_LEN);
    let big = record_with_pins(40); // 40 pins with 40-character addresses
    let padded = seal_pin_record(&big, &mk, 7).len() - NONCE_LEN - TAG_LEN;
    assert!(padded > 256 && padded % 256 == 0, "{padded}");
}

#[test]
fn an_unknown_version_is_unusable() {
    let mk = Secret::random();
    let mut record = sample_record();
    record.version = 2;
    assert_eq!(open_pin_record(&seal_pin_record(&record, &mk, 7), &mk, 7), None);
}

#[test]
fn every_refusal_changes_the_refusal_id_and_retirement_needs_it() {
    let mut r = ZaPinRecord::new();
    let first = r.refuse(5, "u", "bob@example.com", 100, None);
    let second = r.refuse(5, "u", "bob@example.com", 200, None);
    assert_ne!(first, second);
    assert_eq!((r.withheld.len(), r.withheld[0].since, r.withheld[0].refusal_id), (1, 100, second));
    assert!(!r.retire(5, "u", "bob@example.com", first), "a re-armed entry stays");
    assert!(r.retire(5, "u", "bob@example.com", second));
    r.refuse(5, "u", "bob@example.com", 1, None);
    r.refuse(6, "u", "bob@example.com", 1, None);
    r.refuse(5, "v", "bob@example.com", 1, None);
    assert_eq!(r.withheld.len(), 3, "identity is (document_id, uid, address)");
}

#[test]
fn refused_instances_union_and_switch_to_all_past_64_keys() {
    let (mut a, mut b) = (ZaInstances::default(), ZaInstances::default());
    a.add(Some((10, false)));
    b.add(Some((20, true)));
    b.add(None);
    a.union(&b);
    assert!(a.master && a.contains(Some((10, false))) && a.contains(Some((20, true))) && !a.all);
    let mut c = ZaInstances::default();
    c.add(None);
    for i in 0..64 {
        c.add(Some((i, false)));
    }
    assert!(!c.all && c.keys.len() == 64, "the master does not count");
    c.add(Some((64, false)));
    assert!(c.all && c.keys.is_empty() && c.contains(Some((999, true))));
    assert!(c.contains(None));
    assert!(ZaInstances { master: false, keys: vec![], all: true }.contains(None), "all includes the master");
    c.add(Some((65, false)));
    assert!(c.keys.is_empty(), "keys stay empty once all is set");
}

#[test]
fn pin_structs_print_no_address() {
    let text = format!("{:?} {:?} {:?}", sample_record(), sample_record().pins[0], sample_record().withheld[0]);
    assert!(!text.contains("example") && !text.contains('@'), "{text}");
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p vault`
Expected: compile errors naming `display_fingerprint`, `WrapMark`, `pins`.

- [ ] **Step 4: Implement the primitives and field 151**

`seal_pin_record` seals `Zeroizing` plaintext `len (u32 BE) || archive || zeros` padded with `div_ceil(PIN_PAD) * PIN_PAD`, as `seal_bytes_as` pads (but big endian, spec 7). `pins.rs` starts with the SPDX header and a module doc citing spec 4.1 and 7. In `field.rs`, add `ZeroAccessPins = 151` after `ZeroAccessVault` and the arm `PrincipalField::ZeroAccessPins => 151`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p vault`, the formatting check, then the per-task compile check.
Expected: all pass; `clean`.

- [ ] **Step 6: Commit**

```bash
git add crates/vault/src/keys.rs crates/vault/src/pins.rs crates/vault/src/lib.rs crates/types/src/field.rs
git commit -m "Key pinning: fingerprints, wrap marks, the sealed pin record, field 151" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 2: Vault storage layer

**Files:**
- Modify: `crates/common/src/auth/vault.rs` (new items after `vault_write_store`, line 160, and in the `impl Server` block, lines 162-233; unit tests in `mod tests`)
- Modify: `crates/http/src/api/vault.rs:441-472` (`za_commit` wraps `za_vault_commit`)
- Modify: `tests/src/utils/cleanup.rs:382-391` (exempt field 151 with 150)
- Modify: `tests/src/utils/storage.rs:255-265` (the assertion message names vault or pin records)
- Modify: `tests/src/utils/za.rs` (store-writer helpers)
- Modify: `CLAUDE.md` (the vault-write rule in "Where the code lives")

**Interfaces:**
- Consumes: Task 1 (`pins::*`, `keys::{WrapMark, wrap_private_key, unwrap_private_key}`); `VaultRead`, `RawValue`, `vault_read_store`, `is_vault_unusable`, `za_invalidate_account`.
- Produces, in `common::auth::vault`:
  - `pub const ZA_PIN_WRITE_ATTEMPTS: usize = 8;`
  - `pub fn za_normalize_address(address: &str) -> Option<String>`: `utils::sanitize_email`, then `to_canonical_address` (4.3); no `mailto:` decoding, so the endpoints reject that form; the fan-out applies it to the addresses `Email::new` already produced.
  - `pub struct ZaPinRead { pub raw: Option<Vec<u8>>, pub record: Option<ZaPinRecord> }` with `pub fn assertion(&self) -> AssertValue` (`Hash(xxh3_64(raw))` whenever `raw` is `Some`, garbage included; `None` only when absent) and `pub fn is_usable(&self) -> bool`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ZaMark { Pinned, Legacy, Neither }`.
  - `pub struct ZaMarkRead { pub vault: Option<VaultRead>, pub mark: ZaMark, pub private_key: Option<Secret> }`: one vault read; a missing or unusable record is `Neither` with `vault: None`.
  - `pub struct ZaPinState { pub pins: ZaPinRead, pub mark: Option<ZaMarkRead> }`: `mark` is read only when `pins` is not usable (4.4: a record that opens is proof enough).
  - `pub struct ZaPinWrite<'a> { pub previous: &'a ZaPinRead, pub sealed: Vec<u8> }`.
  - `pub enum ZaPinStep<T> { Write(ZaPinRecord, T), Keep(T) }`; `pub enum ZaPinTransact<T> { Written { state: ZaPinState, value: T }, Kept { state: ZaPinState, value: T }, Conflict }`.
  - `impl Server`:
    - `pub async fn za_vault_commit(&self, account_id: u32, record: &mut VaultRecord, previous: Option<&VaultRead>, pins: Option<ZaPinWrite<'_>>) -> trc::Result<()>`: sets `record.revision` to the previous revision + 1 (1 when `None`); one `BatchBuilder` batch on Principal document 0: the vault `assert_value` (`Hash(cas)`, or `None`) then its `set`, and with `pins` field 151's `assert_value(previous.assertion())` then its `set(sealed)`, each assertion before its set (4.4); after a commit, `za_invalidate_account`, a failure logged as `za_invalidate_after_commit` logs it. A lost assertion is `Err(e)` with `e.is_assertion_failure()` and nothing written.
    - `pub async fn za_pin_read(&self, keys: &SessionKeys) -> trc::Result<ZaPinRead>`; `pub async fn za_mark_read(&self, keys: &SessionKeys) -> trc::Result<ZaMarkRead>` (pinned, then legacy, through `unwrap_private_key`); `pub async fn za_pin_state(&self, keys: &SessionKeys) -> trc::Result<ZaPinState>`.
    - `pub async fn za_pin_transact<T: Send>(&self, keys: &SessionKeys, start: Option<ZaPinState>, decide: impl FnMut(&ZaPinState) -> ZaPinStep<T> + Send) -> trc::Result<ZaPinTransact<T>>` (4.2's attempts, below).
    - `pub async fn za_served_public_key(&self, account_id: u32) -> trc::Result<Option<[u8; 32]>>` (5.1): `public_key` when the record reads, is `Active` and holds 32 bytes; `None` for a missing, unusable or pending record; logs nothing (it uses `vault_read_store`, not the logging `vault_read_usable_store`); drops the rest of the record.
  - Store-level twins the unit tests call: `pin_read_store(store, account_id, mk)`, `pin_write_store(store, account_id, write: ZaPinWrite<'_>)` (a field 151 assertion and set only), `vault_commit_store(store, account_id, record, previous, pins)`, `mark_read_store(store, keys)`, `served_public_key_store(store, account_id)`, all `pub(crate)`.
- Produces, in `tests/src/utils/za.rs` (the store writer of spec 2; raw writes are allowed here):
  - `pub fn writer_key() -> [u8; 32]` (a fresh public key, `generate_keypair().0`).
  - `pub async fn substitute_public_key(test: &TestServer, account_id: u32, key: &[u8], invalidate: bool) -> Vec<u8>`: writes `key` as `public_key` at the next revision (`za_vault_write`), with `za_invalidate_account` when `invalidate`; returns the replaced bytes. Restoring is the same call with the original.
  - `pub async fn wrap_mark(test, account_id: u32, password: &str) -> ZaMark` (tries each purpose with `unwrap_key` itself, independent of `unwrap_private_key`, so a mutation there cannot blind it); `pub async fn saved_wrap(test, account_id) -> Vec<u8>`; `pub async fn restore_wrap(test, account_id, wrap: &[u8])` (next revision, invalidates); `pub async fn rewrap_legacy(test, account_id, password)` (next revision, invalidates, as `restore_wrap` does).
  - `pub async fn pin_record(test, account_id, password) -> Option<ZaPinRecord>` (`None`: absent or unusable); `pub async fn write_pin_record(test, account_id, password, record: &ZaPinRecord)`; `pub async fn raw_pins(test, account_id) -> Option<Vec<u8>>`; `pub async fn set_raw_pins(test, account_id, raw: Option<&[u8]>)` (unconditional set, or clear, of field 151).
  - `pub async fn vault_revision(test, account_id) -> u64`; `pub async fn za_unpin(test, account_id, password)`: `rewrap_legacy` plus `set_raw_pins(None)`, the never-pinned state.

`za_pin_transact`, which the signature does not determine:

```text
for attempt in 0..ZA_PIN_WRITE_ATTEMPTS:
    state = start.take() on the first attempt, else self.za_pin_state(keys)
    match decide(&state):
        Keep(v)  => return Kept { state, v }
        Write(mut record, v) =>
            record.revision = state.pins.record.map_or(1, |r| r.revision + 1)
            sealed = seal_pin_record(&record, keys.mk(), id)
            mark = state.mark, or self.za_mark_read(keys) now        // the same VaultRead throughout
            if mark.mark == Legacy:                                   // first pin or heal (4.4)
                vault = mark.vault.record with private_key_wrap = wrap_private_key(key, mk, id, Pinned)
                za_vault_commit(id, vault, Some(&mark.vault), Some(ZaPinWrite { previous: &state.pins, sealed }))
            else:                                                     // pinned, or a usable record under a wrap that opens under neither
                pin_write_store(id, ZaPinWrite { previous: &state.pins, sealed })
            Ok         => return Written { state: ZaPinState { pins: { raw: sealed, record }, mark: None }, v }
            assertion  => continue
            other err  => return Err
return Conflict
```

- [ ] **Step 1: Write the failing unit tests (`EphemeralStore`)**

```rust
#[tokio::test]
async fn pin_writes_assert_absence_or_the_stored_bytes() {
    let (store, mk) = (EphemeralStore::open(), Secret::random());
    let absent = pin_read_store(&store, 5, &mk).await.unwrap();
    assert!(absent.raw.is_none() && matches!(absent.assertion(), AssertValue::None));
    pin_write_store(&store, 5, ZaPinWrite { previous: &absent, sealed: seal_pin_record(&one_pin(), &mk, 5) }).await.unwrap();
    let err = pin_write_store(&store, 5, ZaPinWrite { previous: &absent, sealed: seal_pin_record(&one_pin(), &mk, 5) }).await.unwrap_err();
    assert!(err.is_assertion_failure(), "a second create loses");
    write_raw_pins(&store, 5, b"garbage".to_vec()).await;
    let garbage = pin_read_store(&store, 5, &mk).await.unwrap();
    assert!(garbage.raw.is_some() && !garbage.is_usable());
    pin_write_store(&store, 5, ZaPinWrite { previous: &garbage, sealed: seal_pin_record(&two_pins(), &mk, 5) }).await.unwrap();
    assert_eq!(pin_read_store(&store, 5, &mk).await.unwrap().record, Some(two_pins()));
}

#[tokio::test]
async fn the_first_pin_batch_writes_both_fields_or_neither() {
    // A legacy-wrapped Active record at revision 3, read as `fresh`; `stale` is a read taken at revision 2.
    let err = vault_commit_store(&store, 5, &mut rewrapped(&stale), Some(&stale), Some(pin_write(&absent))).await.unwrap_err();
    assert!(err.is_assertion_failure());
    assert!(pin_read_store(&store, 5, &mk).await.unwrap().raw.is_none(), "no record without the rewrap");
    assert_eq!(mark_read_store(&store, &keys).await.unwrap().mark, ZaMark::Legacy);
    vault_commit_store(&store, 5, &mut rewrapped(&fresh), Some(&fresh), Some(pin_write(&absent))).await.unwrap();
    let mark = mark_read_store(&store, &keys).await.unwrap();
    assert_eq!((mark.mark, mark.vault.unwrap().record.revision), (ZaMark::Pinned, 4));
    assert!(pin_read_store(&store, 5, &mk).await.unwrap().is_usable());
}

#[tokio::test]
async fn the_mark_is_read_pinned_first_then_legacy() {
    // Records with a pinned wrap, a legacy wrap, a wrap of other bytes, and no record.
    assert_eq!(marks, [ZaMark::Pinned, ZaMark::Legacy, ZaMark::Neither, ZaMark::Neither]);
    assert!(neither.private_key.is_none() && missing.vault.is_none());
    assert_eq!(derive_public_key(pinned.private_key.as_ref().unwrap()), public);
}

#[tokio::test]
async fn the_served_key_is_a_usable_public_key_or_nothing() {
    assert_eq!(served_public_key_store(&store, ACTIVE).await.unwrap(), Some(public));
    for id in [PENDING, MISSING, UNUSABLE] {
        assert_eq!(served_public_key_store(&store, id).await.unwrap(), None, "{id}");
    }
}

#[test]
fn addresses_normalize_as_pins_are_keyed() {
    assert_eq!(za_normalize_address(" Bob@Example.COM ").as_deref(), Some("bob@example.com"));
    assert_eq!(za_normalize_address("bob@bücher.test").as_deref(), Some("bob@xn--bcher-kva.test"));
    for bad in ["mailto:bob@example.com", "Bob <bob@example.com>", "bob", ""] {
        assert_eq!(za_normalize_address(bad), None, "{bad}");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p common auth::vault`
Expected: compile errors naming `pin_read_store`, `ZaMark`, `za_normalize_address`.

- [ ] **Step 3: Implement the storage layer**

Field 151's key is `ValueKey::property(account_id, Collection::Principal, 0, PrincipalField::ZeroAccessPins)` beside `vault_key`. `za_commit` keeps its signature and its expected-revision 409 and `za_prune`, then calls `server.za_vault_commit(account_id, record, previous, None)`, mapping an assertion failure to `conflict("conflict")`; its own `za_invalidate_after_commit` call goes (the helper invalidates). The doc comment on `za_vault_write` says product code calls `za_vault_commit`.

- [ ] **Step 4: The cleanup exemption and the helpers**

`cleanup.rs`: the `SUBSPACE_PROPERTY` arm matches `key[U32_LEN + 1]` equal to `ZeroAccessVault as u8` or `ZeroAccessPins as u8`, so `assert_is_empty` (za suite, and key-mode `webdav_tests`, whose sub-modules call it while their accounts exist) accepts a pin record exactly while its account exists. `storage.rs`: the panic message becomes "Found a zero-access vault or pin record of destroyed account {account_id}". `danger_destroy_account` removes the account's whole key range per subspace (`crates/store/src/dispatch/store.rs:344-390`), field 151 included, so the store needs no change (4.2). Write the `za.rs` helpers of the Interfaces block.

- [ ] **Step 5: `CLAUDE.md`**

In "Where the code lives", the sentence "Every vault write goes through `za_commit`; never call `za_vault_write` directly." becomes "Every vault write goes through `za_commit` or the common helper it wraps, `Server::za_vault_commit` (`crates/common/src/auth/vault.rs`, key-pinning spec 4.5); never call `za_vault_write` directly."

- [ ] **Step 6: Run the tests and checks**

Run: `cargo test -p common auth::vault`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite; `ZA_KEY_ACCOUNTS=1` `webdav_tests`.
Expected, in order: pass; no formatting difference; `clean`; clean; the za suite passes unchanged; key-mode `webdav_tests` passes unchanged (nothing writes field 151 yet).

- [ ] **Step 7: Commit**

```bash
git add crates/common/src/auth/vault.rs crates/http/src/api/vault.rs tests/src/utils/cleanup.rs tests/src/utils/storage.rs tests/src/utils/za.rs CLAUDE.md
git commit -m "Key pinning: the common vault commit helper, pin record reads and conditional writes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 3: The self-check

**Files:**
- Modify: `crates/common/src/auth/vault.rs:320-362` (`za_open_private_key` is replaced by the store-level `self_check_store`), `:470-477` (`za_private_key`), unit tests at `:889-969`
- Modify: `crates/groupware/src/calendar/drain.rs:128-140` (`PassOutcome` pub), `:150-171` (`za_drain`), `:183-222` (`drain_pass`)
- Modify: `tests/src/utils/za.rs:575-582` (`open_pending_item_as` unwraps read-only)
- Create: `tests/src/za/pinning.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod pinning;`; `pinning::test(&mut test).await;` after `drain_limits::test`)

**Interfaces:**
- Consumes: Task 2 (`za_mark_read`, `za_pin_read`, `za_vault_commit`, `ZaPinWrite`, `ZaMark`, `ZA_PIN_WRITE_ATTEMPTS`, the `za.rs` helpers); Task 1 (`derive_public_key`, `wrap_private_key`, `seal_pin_record`).
- Produces:
  - `pub struct ZaSelfCheck { pub stored_key_matches: bool, pub public_key: [u8; 32] }` (`stored_key_matches` as the first read found it, before any repair, 6.2; `public_key` derived, the own fingerprint's source, 3.2).
  - `pub(crate) async fn self_check_store(store: &Store, keys: &SessionKeys, heal: bool, once: bool) -> trc::Result<(ZaSelfCheck, bool)>`: the algorithm below over the store-level twins of Task 2 (`mark_read_store`, `pin_read_store`, `vault_commit_store`); the `bool` it returns says whether it committed a write. It replaces `za_open_private_key`. `once` is the once-per-session gate that function has today: with `once`, a recorded outcome is returned without a store read (`Ok`: `stored_key_matches: true` and the key derived from the recorded private key; a recorded failure: `Err`). The drain passes `once: true`. The contacts endpoints pass `once: false` and always run the full check, because `authenticate_with_keys` (`common/src/auth/authentication.rs:57-73`) can hand them `SessionKeys` a DAV session of the same password already cached and checked (case 7's "the listing repairs it" needs this); on keys whose private key is already recorded it compares, repairs and heals as usual and leaves the recorded outcome as it is.
  - `Server::za_self_check(&self, keys: &SessionKeys, heal: bool, once: bool) -> trc::Result<ZaSelfCheck>`: `self_check_store`, then `za_invalidate_account` when it wrote. It records the unwrap outcome in `keys` as `za_open_private_key` did, `PRIVATE_KEY_FAIL_ONCE` included. A wrap that opens under neither, or a missing or unusable record, is a recorded failure (traced once, `Err`, `keys.private_key()` is `Some(Err)`). A store error after the key opened (the pin read, the repair or heal write) leaves the key recorded `Ok` and returns the error; one on the first vault read records nothing. `heal` is `true` on the drain's first pass and `false` for endpoints, which heal only by writing the record (4.4).
  - `Server::za_private_key` keeps its signature; it runs `za_self_check(keys, true, true)` (a recorded outcome returns at once), and a store error with the key recorded is traced (`zero-access: self-check failed`) and the key returned (spec 8: the drain proceeds).
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PassOutcome { Complete, LeftPending, More, Suspended }` and `ZaDrain::za_drain(&self, keys: &Arc<SessionKeys>) -> impl Future<Output = PassOutcome> + Send`: the pass's outcome; an error (traced as today) is `LeftPending`; a deferral in force is `LeftPending`, or `Suspended` when permanent; a cursor at the snapshot is `Complete`; the test-mode drain skip is `LeftPending`. `handle_dav_request`'s statement `self.za_drain(keys).await;` stays as it is.
  - `open_pending_item_as` reads the recipient's vault record and opens its wrap with `unwrap_private_key(&wrap, keys.mk(), recipient)`: no outcome recorded, no write, so a test read never repairs or heals.
  - `tests/src/za/pinning.rs`: constants `ALICE = "key1@example.com"`, `BOB = "key2@example.com"`, `CAROL = "key3@example.com"`; `fn parties(test) -> (Party, Party, Party)`; `async fn rewrite(client: &DummyWebDavClient, href: &str, edit: impl FnOnce(String) -> String) -> StatusCode` (GET, edit, PUT with `CONTENT_TYPE`, as `set_partstat` does); `fn record_pinning(address: &str, key: [u8; 32]) -> ZaPinRecord` (one `FirstUse` pin); `async fn withheld(test, account_id, address) -> Vec<ZaWithheld>` (through `pin_record` with `STRONG`); `async fn served(test, account_id) -> [u8; 32]`; `pub async fn test(test: &mut TestServer)` holding `DrainOn`.

The self-check, which the signature does not determine (3.3):

```text
if once and keys.private_key() is Some: return the recorded outcome, no store read
for attempt in 0..ZA_PIN_WRITE_ATTEMPTS:              // "record" below: set_private_key, a no-op once recorded
    m = za_mark_read(keys)                               // one vault read
    Neither -> record Err(Aead) (traced once), return Err
    derived = derive_public_key(m.private_key); matches = m.vault.public_key == derived
    on the first attempt: report.stored_key_matches = matches
    pins = za_pin_read(keys)                             // the record, read once per attempt
    heal = m.mark == Legacy && pins.is_usable() && (heal || !matches)
    if matches && !heal: record Ok(key); return Ok(report)
    if !matches, once per call: trace Security(Unauthorized) "zero-access: stored public key does not match the private key"
    vault' = m.vault.record with public_key = derived (when !matches) and private_key_wrap pinned (when heal)
    pins'  = when pins.is_usable(): the record with revision + 1 (4.1), key_repaired_at = Some(now) when !matches, sealed
             otherwise nothing: the self-check never creates a record (invariant 22)
    za_vault_commit(id, vault', Some(&m.vault), pins'.map(..)):
        Ok -> record Ok(key); return Ok(report)
        assertion -> continue                          // re-read and re-check; a key that now matches writes nothing
        other -> record Ok(key); return Err(err)
record Ok(key); return Ok(report)                      // the next session checks again
```

`drain_pass` runs it first, after the deferral check: when `keys.private_key()` is `None` it calls `za_private_key(keys)`; a recorded failure defers permanently and returns `Suspended`; any other error is traced and the pass goes on (spec 3.3: every session's first pass runs the check, items pending or not). `drain_pass` returns `trc::Result<PassOutcome>`.

- [ ] **Step 1: Adapt the two unit tests and write case 3**

`private_key_unwraps_from_the_record_once_per_session` and `a_private_key_wrap_that_does_not_open_is_an_error_and_no_key` move from `za_open_private_key` to `self_check_store(&store, &keys, false, true)` (the gated path), with their assertions unchanged in substance: each one that read the returned key reads `keys.private_key()` after the call. One new unit test covers the ungated path:

```rust
#[tokio::test]
async fn an_ungated_self_check_reads_the_store_again() {
    // an Active record with a matching public key; keys recorded by a first gated call
    self_check_store(&store, &keys, false, true).await.unwrap();
    write_public_key(&store, 3, &writer).await; // the stored key replaced at the next revision
    let (check, wrote) = self_check_store(&store, &keys, false, true).await.unwrap();
    assert!(check.stored_key_matches && !wrote, "gated: the recorded outcome, no read");
    let (check, wrote) = self_check_store(&store, &keys, false, false).await.unwrap();
    assert!(!check.stored_key_matches && wrote, "ungated: compared and repaired");
    assert_eq!(vault_read_store(&store, 3).await.unwrap().unwrap().record.public_key, public.to_vec());
    assert_eq!(keys.private_key().unwrap().unwrap().as_bytes(), private.as_bytes(), "the recorded outcome stays");
}
```

`case_03_repair` (bob, no item pending):

```rust
za_unpin(test, bob.id, STRONG).await;
write_pin_record(test, bob.id, STRONG, &record_pinning(ALICE, served(test, alice.id).await)).await;
let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
let revision = vault_revision(test, bob.id).await;
assert!(pending_ids(test, bob.id).await.is_empty());
assert_eq!(trigger_drain(BOB, STRONG).await, 207);
let stored = test.server.za_vault_record(bob.id).await.unwrap().unwrap().record.public_key;
assert_eq!(stored, original, "the stored key is rewritten from the private key");
assert_eq!(vault_revision(test, bob.id).await, revision + 1, "one batch: the repair and the heal");
assert_eq!(wrap_mark(test, bob.id, STRONG).await, ZaMark::Pinned);
let record = pin_record(test, bob.id, STRONG).await.expect("the record survives");
assert!(record.key_repaired_at.is_some());
assert!(record.pin(ALICE).is_some());
assert_eq!(trigger_drain(BOB, STRONG).await, 207);
assert_eq!(vault_revision(test, bob.id).await, revision + 1, "a matching key writes nothing");
```

- [ ] **Step 2: Run the za suite to see case 3 fail**

Run: the za suite.
Expected: FAIL in `pinning` at "the stored key is rewritten from the private key".

- [ ] **Step 3: Implement the self-check and the drain changes**

As above. The module doc of `drain.rs` gains one sentence: the first pass of every session runs the self-check (key-pinning spec 3.3) before enumerating.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p common auth::vault`; `cargo test -p groupware`; the formatting check; the per-task compile check; the za suite; `ZA_KEY_ACCOUNTS=1` `webdav_tests`.
Expected: pass (`drain_limits` still passes: a recorded private-key failure still suspends, now at the first pass); `clean`.

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/auth/vault.rs crates/groupware/src/calendar/drain.rs tests/src/utils/za.rs tests/src/za/pinning.rs tests/src/za/mod.rs
git commit -m "Key pinning: the self-check on every session's first drain pass" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 4: Pin decisions in the fan-out

**Files:**
- Modify: `crates/groupware/src/calendar/fanout.rs` (module doc; the `ZaFanOut` trait and impl, lines 93-210, into the four phases; `resolve_inner`, lines 442-472; `status_rank`, lines 394-400; new constant, types and `refused_instances`; unit tests)
- Modify: `crates/groupware/src/calendar/drain.rs:790-804` (`InstanceKey`, `instance_key` become `pub`), `:1083-1090` (the delegating merge's call)
- Modify: `crates/groupware/src/calendar/storage.rs:594-598` (`ZaDeleteMessage` source fields)
- Modify: `crates/dav/src/calendar/update.rs:347-357` (update call), `:540-575` and `:586-590` (the create assigns its document id before the fan-out)
- Modify: `crates/dav/src/calendar/delete.rs:252-264`
- Modify: `crates/common/src/za/test_hooks.rs` (three hooks, module doc)
- Modify: `tests/src/za/pinning.rs` (cases 1, 9, 16; `rf_2_mixed_recipients`, `rf_3_concurrent_first_use`)

**Interfaces:**
- Consumes: Task 1 (`ZaPinRecord::{new, pin, set_pin, refuse, retire}`, `ZaInstances`, `PinSource`); Task 2 (`za_pin_state`, `za_pin_transact`, `ZaPinState`, `ZaPinStep`, `ZaPinTransact`, `ZaMark`, `za_served_public_key`, `za_normalize_address`); `HookParked` (`tests/src/utils/za.rs`).
- Produces, in `groupware::calendar::fanout`:
  - `pub const ITIP_STATUS_KEY_REFUSED: &str = "3.8";`; `status_rank` returns 3 for it.
  - `#[derive(Clone, Copy)] pub struct ZaSource<'a> { pub document_id: u32, pub uid: &'a str }`.
  - `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ZaFanOutKind { Change, Deletion, Redelivery }`.
  - `fn za_fan_out(&self, sender: &AccountInfo, keys: &SessionKeys, messages: Vec<ItipMessage<ICalendar>>, stored_pairs: Option<TokenPairs>, event: &ICalendar, source: ZaSource<'_>, kind: ZaFanOutKind, fence: Option<&ZaUidLease>) -> impl Future<Output = trc::Result<ZaFanOutResult>> + Send`. `Err` still only for foreign keys and a lost fence (5.4: a pin failure is a status, never an `Err`).
  - `ZaDeleteMessage` gains `pub document_id: u32` and `pub uid: String` (the deleted event's).
  - `fn refused_instances(reply: &ICalendar, addresses: &[String]) -> ZaInstances` (private): `master` for a component without RECURRENCE-ID, `instance_key` of each override from the REPLY's own snapshot (its own VTIMEZONEs, 4.1).
- Produces, in `common::za::test_hooks` (module doc gains three bullets): `PIN_DECIDE_PAUSE` (`set_pin_decide_pause(Option<Arc<Pause>>)`, `pin_decide_pause_point(sender_id)`: a fan-out after its phase 1 reads); `RETIRE_PAUSE` (`set_retire_pause`, `retire_pause_point(sender_id)`: after phase 3's items, before phase 4, reached by `contacts/pin`'s inner call too); `PIN_WRITE_FAIL_ONCE` (`set_pin_write_fail(Option<u32>)`, `take_pin_write_fail(sender_id) -> bool`: the phase 2 write fails once with an internal error).
- Produces, in `pinning.rs`: `fn invite_from(organizer: &str, uid: &str, start: &str, attendees: &[&str]) -> String` (`fanout::invite` with that ORGANIZER); `fn put_raw(name: &str, path: &str, body: String) -> impl Future<Output = u16> + Send + 'static` (through `HttpRequest`, as `trigger_drain` sends); `async fn document_id_of(test: &TestServer, account_id: u32, href: &str) -> u32` (the event's document id, from the resource cache as `copy_href` reads it).

The phases (spec 5.1 table, 5.2), which the signature does not determine:

```text
Phase 1, resolve and read:
  per message in order, per address: route() as today; a Deliver route resolves once per
    za_normalize_address(address) (an unsanitizable address is not internal): za_account_id_from_email,
    not the sender, is_key_account (account cache), CalendarSchedulingReceive, then za_served_public_key
    (None: not internal, 5.3; Err: 5.1, traced without the recipient). The cached za_public_key is not used.
  any Internal recipient (or kind == Deletion with a message): state = za_pin_state(keys)
    Err: every Internal recipient 5.1, nothing written for them, traced "pin record read failed"
  pin_decide_pause_point(sender_id)
Phase 2, decide and write once (take_pin_write_fail first: a failed write):
  za_pin_transact(keys, Some(state), decide), decide over every Internal recipient:
    usable record:   pin == served -> Deliver | pin != served -> Refuse | no pin -> set_pin(FirstUse), Deliver
    absent/unusable: mark Legacy -> ZaPinRecord::new() with FirstUse pins for all, Deliver all
                     mark Pinned or Neither -> Refuse all and record nothing (Keep): fail closed (4.4)
    each Refuse outside fail-closed and outside kind == Deletion:
      record.refuse(source.document_id, source.uid, address, now, Some(&refused_instances(..)) for a REPLY)
    Write when a pin or an entry changed, else Keep; the value carries the decisions and, for each
      Deliver recipient with an entry (source, address) in the state this attempt decided on, its refusal_id
  Err or Conflict: every Internal recipient 5.1, nothing written for them, traced "pin record write failed"
  any Refuse: one refusal trace (Total = refused recipients)
Phase 3, items: per (message, address) in message order: Deliver -> write_item with the served key;
  Refuse -> record_status(.., ITIP_STATUS_KEY_REFUSED) on the routed property; the rest as today.
  Then retire_pause_point(sender_id).
Phase 4, kind == Change only: the Deliver recipients holding a consumed id whose items were all written:
  za_pin_transact(keys, Some(the Written or Kept state), retire), retire = record.retire(.., consumed)
  for each, checked on every attempt; Err or Conflict traced "withheld entries not retired", ignored.
```

The pin-record writes carry no UID lease fence (5.5); `fence` still guards item writes only. Call sites: the update PUT passes `ZaSource { document_id, uid }` (the stored UID, which `update.rs:207-213` validated equal to the new one) and `Change`; the create moves its `assign_document_ids` call (lines 586-590) to just before `if let Some(keys) = &za_keys` (line 540), unchanged for plain accounts, and passes `{ document_id, uid }`, the `uid` being the lease's `String` (`za_lease`'s second element; the tree was moved into the event at line 461), and `Change` with its fence as today; `delete.rs` passes `ZaSource { document_id: message.document_id, uid: &message.uid }` and `Deletion`; the delegating merge passes `ZaSource { document_id: event_id, uid: &event_uid }` and `Change`.

- [ ] **Step 1: Write the failing unit tests in `fanout.rs`**

```rust
#[test]
fn a_key_refusal_outranks_every_other_status() {
    let mut statuses = Vec::new();
    for status in [ITIP_STATUS_DELIVERY_FAILED, ITIP_STATUS_KEY_REFUSED, ITIP_STATUS_DELIVERED, ITIP_STATUS_NOT_ATTEMPTED] {
        record_status(&mut statuses, "bob@example.com", false, status);
    }
    assert_eq!(statuses[0].status, "3.8");
    assert!(status_rank(ITIP_STATUS_KEY_REFUSED) > status_rank(ITIP_STATUS_DELIVERY_FAILED));
}

#[test]
fn a_later_delivery_replaces_a_key_refusal() {
    // status_of(address, status): a test-local ZaScheduleStatus with on_organizer: false.
    // EVENT's carol line already carries SCHEDULE-STATUS=3.7.
    let mut event = parse(EVENT);
    za_set_schedule_status(&mut event, &[status_of("bob@example.com", ITIP_STATUS_KEY_REFUSED)]);
    za_set_schedule_status(&mut event, &[status_of("bob@example.com", ITIP_STATUS_DELIVERED)]);
    let attendees = statuses_of(&event, ICalendarProperty::Attendee);
    assert_eq!(attendees[1].1, ["1.2"]);
    assert_eq!(attendees[2].1, ["3.7"], "upstream's 3.7 stays");
}

#[test]
fn refused_instances_are_recorded_by_instant() {
    // bob's REPLYs: (a) RECURRENCE-ID;TZID=Fixed-Plus-One:20990302T110000 with a VTIMEZONE of one
    // STANDARD at TZOFFSETTO:+0100; (b) RECURRENCE-ID:20990302T100000Z; (c) the master only.
    assert_eq!(refused_instances(&tzid, &bob()).keys, refused_instances(&utc, &bob()).keys);
    assert_eq!(refused_instances(&tzid, &bob()).keys.len(), 1);
    assert_eq!(refused_instances(&master, &bob()), ZaInstances { master: true, keys: vec![], all: false });
}
```

- [ ] **Step 2: Write the failing integration cases**

`case_01_first_use_under_a_legacy_wrap`: `za_unpin(alice)`; `revision = vault_revision(alice)`; alice PUTs `invite("pin-01", START_A, &[BOB])` (201).

```rust
assert_eq!(new_inbox_items(test, bob.id, &before).await.len(), 1);
let record = pin_record(test, alice.id, STRONG).await.expect("the first pin creates the record");
let pin = record.pin(BOB).expect("bob is pinned");
assert_eq!((pin.source, pin.public_key), (PinSource::FirstUse, served(test, bob.id).await));
assert!(record.withheld.is_empty());
assert_eq!(wrap_mark(test, alice.id, STRONG).await, ZaMark::Pinned);
assert_eq!(vault_revision(test, alice.id).await, revision + 1, "one batch, one bump");
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
// alice invites carol ("pin-01b"): carol is pinned and the vault is not written
assert!(pin_record(test, alice.id, STRONG).await.unwrap().pin(CAROL).is_some());
assert_eq!(vault_revision(test, alice.id).await, revision + 1, "later pin writes leave the vault alone");
```

`case_09_healing`: carol holds a record pinning alice (`za_unpin(carol)`, then carol invites alice with `invite_from`). (a) The fan-out: `rewrap_legacy(carol)`; `test_hooks::set_drain_skip(carol.id)` so no drain heals first; carol invites bob (a new pin, so the record is written); `clear_drain_skip(carol.id)`. (b) The drain: `rewrap_legacy(carol)` again, then `trigger_drain(CAROL, STRONG)`. Then fail closed: `set_raw_pins(carol, None)` and carol edits the bob invitation.

```rust
// after (a)
assert_eq!(wrap_mark(test, carol.id, STRONG).await, ZaMark::Pinned);
assert_eq!(vault_revision(test, carol.id).await, revision_a + 1, "one batch: the record and the rewrap");
let record = pin_record(test, carol.id, STRONG).await.unwrap();
assert!(record.pin(ALICE).is_some() && record.pin(BOB).is_some());
// after (b)
assert_eq!(wrap_mark(test, carol.id, STRONG).await, ZaMark::Pinned);
assert_eq!(vault_revision(test, carol.id).await, revision_b + 1);
assert_eq!(pin_record(test, carol.id, STRONG).await.unwrap().pins, record.pins, "a heal changes no pin");
// after the record is deleted
assert_eq!(attendee_statuses(&carol.client, &path, BOB).await, ["3.8"]);
assert!(new_inbox_items(test, bob.id, &before).await.is_empty());
assert_eq!(raw_pins(test, carol.id).await, None, "failing closed writes no record");
```

Cleanup ends with `za_unpin(carol)`.

`case_16_pin_before_deliver`: alice holds a record (`za_unpin`, then an invitation to bob); `set_pin_write_fail(Some(alice.id))`; alice invites `key3+pin16@example.com`, a new contact; `set_pin_write_fail(None)`.

```rust
assert_eq!(attendee_statuses(&alice.client, &path, "key3+pin16@example.com").await, ["5.1"]);
assert!(new_inbox_items(test, carol.id, &before).await.is_empty(), "no item without its pin");
assert!(pin_record(test, alice.id, STRONG).await.unwrap().pin("key3+pin16@example.com").is_none());
// alice's next edit pins and delivers
assert_eq!(attendee_statuses(&alice.client, &path, "key3+pin16@example.com").await, ["1.2"]);
assert_eq!(new_inbox_items(test, carol.id, &before).await.len(), 1);
```

`rf_2_mixed_recipients` (Review Focus 2): alice invites bob and carol; bob's key substituted (`invalidate: true`); alice edits SUMMARY.

```rust
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
assert_eq!(attendee_statuses(&alice.client, &path, CAROL).await, ["1.2"]);
assert!(new_inbox_items(test, bob.id, &bob_before).await.is_empty());
assert_eq!(new_inbox_items(test, carol.id, &carol_before).await.len(), 1);
assert_eq!((withheld(test, alice.id, BOB).await.len(), withheld(test, alice.id, CAROL).await.len()), (1, 0));
// bob's key restored, alice edits again: delivered and the entry retired
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
assert!(withheld(test, alice.id, BOB).await.is_empty());
```

`rf_3_concurrent_first_use` (Review Focus 3): alice holds a record pinning bob only. Park `put_raw(ALICE, "pin-rf3a", invite("pin-rf3a", START_A, &[CAROL]))` with `HookParked::start(alice.id, test_hooks::set_pin_decide_pause, ..)`; a second PUT, `invite("pin-rf3b", START_B, &["key2+rf3@example.com"])`, completes (201); finish the parked one.

```rust
assert_eq!(parked.finish().await, 201);
let record = pin_record(test, alice.id, STRONG).await.unwrap();
assert!(record.pin(CAROL).is_some() && record.pin("key2+rf3@example.com").is_some(), "the loser re-read and re-decided");
assert_eq!(attendee_statuses(&alice.client, &path_a, CAROL).await, ["1.2"]);
assert_eq!(attendee_statuses(&alice.client, &path_b, "key2+rf3@example.com").await, ["1.2"]);
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p groupware`, then the za suite.
Expected: compile errors naming `ITIP_STATUS_KEY_REFUSED`, `refused_instances`, `set_pin_write_fail`.

- [ ] **Step 4: Implement the hooks, the phases and the call sites**

As above; the module doc of `fanout.rs` gains a paragraph on the pin check (key-pinning spec 5) and the four phases; `ZaScheduleStatus.status`'s doc names `ITIP_STATUS_KEY_REFUSED`.

- [ ] **Step 5: Run everything this task touches**

Run: `cargo test -p groupware`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite; `webdav_tests` plain (the create's `assign_document_ids` moved for plain accounts too) and with `ZA_KEY_ACCOUNTS=1`.
Expected: pass; `clean`. Earlier za modules now create pin records at first use (the generation-bump rule of Global Constraints applies to any assertion that changes).

- [ ] **Step 6: Commit**

```bash
git add crates/groupware/src/calendar/fanout.rs crates/groupware/src/calendar/drain.rs crates/groupware/src/calendar/storage.rs crates/dav/src/calendar/update.rs crates/dav/src/calendar/delete.rs crates/common/src/za/test_hooks.rs tests/src/za/pinning.rs
git commit -m "Key pinning: the fan-out pins at first use and refuses a changed key with 3.8" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 5: The four endpoints

**Files:**
- Create: `crates/http/src/api/contacts.rs`
- Modify: `crates/http/src/api/mod.rs:14` (`pub mod contacts;` after `pub mod vault;`)
- Modify: `crates/http/src/api/vault.rs:299-309` (four routes in `handle_vault_request`), `:107-160` and `:185-233` (`CredentialsRequest` and its fields, `deserialize_some`, `ok`, `json`, `conflict`, `bad_request` become `pub(super)`), `:516-562` (`ZaVerified` keeps the access token; it and `za_verify_primary` become `pub(super)`)
- Create: `crates/groupware/src/calendar/pinning.rs`
- Modify: `crates/groupware/src/calendar/mod.rs:7-15` (`pub mod pinning;`)
- Modify: `crates/common/src/za/test_hooks.rs` (`REDELIVER_CLEANUP_PAUSE`)
- Modify: `tests/src/za/pinning.rs` (endpoint helpers; cases 2, 4, 6, 7, 8, 10, 14, 15, 17, 18, 19, 20; `rf_1_address_forms`, `rf_4_moved_source`, `rf_5_never_pinned`)

**Interfaces:**
- Consumes: Tasks 1-4; `PassOutcome` and `za_drain` (Task 3); `za_self_check(keys, false, false)`; `ItipSendStatus::resolve` (`crates/groupware/src/calendar/itip.rs:1073`); `organizer_request_full`, `itip_snapshot`, `itip_build_envelope`, `itip_export_component`, `ItipExportAs`, `itip_add_tz`; `ZaUidLease::acquire`, `ZaUidLeaseGuard`; `unseal_event_pairs`, `event_token_pairs`, `seal_event`, `za_set_schedule_status`; `CalendarEvent::update` (asserts the stored archive, `common/src/storage/index.rs:351`).
- Produces, in `groupware::calendar::pinning`:
  - `pub fn za_full_request_for(tree: &ICalendar, addresses: &[String], recipient: &str) -> Option<ItipMessage<ICalendar>>`: `organizer_request_full(tree, &snapshot, None, false)` (no `itip_finalize`, no SEQUENCE bump) with `to` restricted to `recipient` (compared normalized); `None` when it errs or does not reach the recipient (DECLINED without SCHEDULE-FORCE-SEND, RSVP=FALSE, SCHEDULE-AGENT=CLIENT or NONE).
  - `pub fn za_participation_reply(tree: &ICalendar, addresses: &[String], instances: &ZaInstances, from: &str) -> Option<ItipMessage<ICalendar>>` (6.4 step 2): a REPLY to the ORGANIZER of every recorded instance (every instance under `all`) present in `tree` whose local attendee `is_server_scheduling` and has exactly the address `from`, whose ORGANIZER is not SCHEDULE-AGENT=CLIENT and whose STATUS is not CANCELLED; each exported with `itip_export_component(.., ItipExportAs::Attendee(ids))`, `ids` the local attendee's entry plus the attendees delegated from or to it (as `attendee.rs:120-142` collects them); RSVP not consulted; `None` when nothing qualifies.
  - `pub fn za_reply_from(tree: &ICalendar, addresses: &[String], instances: &ZaInstances) -> Option<String>`: the local attendee address of the first recorded instance present, the master first.
- Produces, in `http::api::contacts`: `pub const REDELIVER_BATCH: usize = 32;` and `pub(super) async fn za_contacts(server: &Server, session: &HttpSessionData, request: CredentialsRequest) -> trc::Result<HttpResponse>`, with `za_contacts_lookup`, `za_contacts_pin`, `za_contacts_forget` of the same shape over `LookupRequest { username, password, totp, address: String }`, `PinRequest { .., address: String, fingerprint: String }`, `ForgetRequest { .., address: Option<Option<String>> }` (`deserialize_some`: the key is required, null allowed). Responses: spec 6.2-6.5 field for field (`source` is `"first-use"` or `"confirmed"`).
- Produces, in `http::api::vault`: `pub(super) struct ZaVerified { pub(super) account_id: u32, pub(super) keys: Arc<SessionKeys>, pub(super) access_token: AccessToken }` (the token `authenticate_with_keys` returned, for the SendSealed preconditions of step 1).
- Produces, in `test_hooks`: `REDELIVER_CLEANUP_PAUSE` (`set_redeliver_cleanup_pause`, `redeliver_cleanup_pause_point(account_id)`: `contacts/pin` after its last event commit, before step 5).
- Produces, in `pinning.rs`: `contacts(name) -> Value` (200 expected), `lookup(name, address) -> VaultReply`, `pin(name, address, fingerprint) -> VaultReply`, `forget(name, address: Option<&str>) -> VaultReply` (`{"address": null}` for `None`), all with `STRONG`; `fn listed<'a>(listing: &'a Value, address: &str) -> &'a Value`; `refuse_once_at(test, organizer: &Party, attendee: &Party, address: &str, uid: &str)` and `refuse_once(..)` = `refuse_once_at(.., attendee.name, ..)`, both `-> (String, Vec<u8>, [u8; 32])` (the organizer invites `address`, the attendee drains, the attendee's key is substituted with invalidation and the organizer edits SUMMARY to `v2`: returns the event's href, the original key and the writer's key); `drop_entries(name, address)` (after a case deleted its events: `lookup` then `pin`, which drops entries of gone documents); `vevent_count(client, href) -> usize`; `stored_key(test, account_id) -> Vec<u8>` (the vault record's `public_key`); `opened_summaries(test, account_id, before: &[u32]) -> Vec<String>` (the SUMMARY of each new item, opened with `open_pending_item_as`, sorted); `const NEW: &str = "another horse battery staple 2"`.

Every endpoint, after `za_verify_primary`, runs `za_self_check(&verified.keys, false, false)` (always the full check): a recorded failure (`keys.private_key()` is `Some(Err)`) answers 409 `vault record missing`; any other error propagates (500). An address goes through `za_normalize_address` (`None`: 400, `bad_request("invalid address")`; no 400 detail echoes the input). "Resolves" means `za_account_id_from_email`, `za_is_key_account` and `za_served_public_key`, with no receive-permission check (6.3), else 409 `not a key account`, not counted as a failed authentication. `contacts` answers `fingerprint` = `display_fingerprint(&check.public_key)` and `stored_key_matches` from the self-check, and reads `za_pin_read` and `za_mark_read` after it: `record_missing = mark == Pinned && !pins.is_usable()`, `key_repaired_at` from a usable record; one object per pin and per unpinned address holding entries, sorted by address. `contacts/forget` reads the mark once after the self-check and runs one transact: usable record and an address: remove the pin, `Write` when one was removed or the mark is `Legacy` (the heal batch), else `Keep`; usable and null: `Write` an empty record keeping `key_repaired_at`; absent or unusable under `Legacy`: `Keep`; under `Pinned`: null writes `ZaPinRecord::new()` asserting the bytes read, an address keeps. `Conflict` is 409 `conflict`.

`contacts/pin`, which the signatures do not determine (6.4):

```text
parse_fingerprint(fingerprint) (None: 400, bad_request("invalid fingerprint")); resolve (409); fingerprint_bytes(served) != given: 409 "fingerprint mismatch"
transact: (usable record, or ZaPinRecord::new()).set_pin(address, served, Confirmed, now) -> Write; Conflict: 409
loop { outcome = za_drain(&keys); if outcome != More { break } }; incomplete = outcome in {LeftPending, Suspended}
fresh za_pin_read: the address's entries in record order, the first REDELIVER_BATCH with their refusal_id; the rest remaining
per entry:
  1. archive (account_id, CalendarEvent, document_id), unseal_event_pairs; dropped when: gone, another UID, the tree
     no longer names the address (an ATTENDEE when the ORGANIZER is one of account_info.addresses(), else the
     ORGANIZER), or ItipSendStatus::resolve(.., &verified.access_token, .., range_end, true) is not SendSealed
  2. organizer side: message = za_full_request_for (None: dropped); ZaUidLease::acquire (None: failed)
     attendee side: incomplete: remaining; from = za_reply_from, message = za_participation_reply (None: dropped)
  3. za_fan_out(&account_info, &keys, vec![message], pairs, &tree, ZaSource { document_id, uid }, Redelivery, lease)
     the address's status: 1.2 go on | 5.1 failed | 5.3 dropped | 3.8 remaining (the fan-out re-armed it)
     a lost fence: lease abandoned, failed
  4. za_set_schedule_status, seal_event with the returned pairs, CalendarEvent::update on the read archive, commit
     (schedule tag unchanged); a lost archive assertion: re-read the archive, re-derive (its own event_token_pairs),
     fresh pin read: equal refusal_id -> apply 1.2 and commit once more, different -> skip; remaining either way
     release the lease
redeliver_cleanup_pause_point(account_id)
5. transact over delivered and dropped entries: retire(.., consumed) on every attempt; one whose id differs is remaining
200 {"ok": true, "delivered", "failed", "dropped", "remaining"}
```

- [ ] **Step 1: Write the failing unit tests in `pinning.rs` (groupware)**

```rust
#[test]
fn the_participation_reply_exports_only_recorded_qualifying_instances() {
    // bob's copy (addresses key2@example.com and bob.alias@example.com): master ACCEPTED; A TENTATIVE
    // with RSVP=FALSE; B DECLINED; C STATUS:CANCELLED naming bob; D with bob SCHEDULE-AGENT=CLIENT;
    // E naming bob.alias@example.com.
    let reply = za_participation_reply(&copy, &bob(), &recorded(&[A, C, D, E]), BOB).expect("A qualifies");
    assert_eq!(reply_instances(&reply), [Some(A)]);
    assert_eq!(reply_partstat(&reply, Some(A), BOB).as_deref(), Some("TENTATIVE"), "RSVP is not consulted");
    assert_eq!((reply.from.as_str(), reply.to.clone()), (BOB, vec![ALICE.to_string()]));
    let every = ZaInstances { master: false, keys: vec![], all: true };
    assert_eq!(reply_instances(&za_participation_reply(&copy, &bob(), &every, BOB).unwrap()), [None, Some(A), Some(B)]);
    assert!(za_participation_reply(&copy, &bob(), &recorded(&[C, D, E]), BOB).is_none());
}

#[test]
fn the_participation_reply_carries_the_delegation_lines() {
    // bob DELEGATED, DELEGATED-TO carol on the master; carol DELEGATED-FROM bob.
    let text = za_participation_reply(&delegated, &bob(), &master_only(), BOB).unwrap().message.to_string();
    assert!(text.contains("DELEGATED-TO") && text.contains("DELEGATED-FROM"), "{text}");
}

#[test]
fn the_full_request_names_only_its_recipient() {
    let message = za_full_request_for(&organized, &alice(), BOB).unwrap();
    assert_eq!((message.to.clone(), message.from_organizer), (vec![BOB.to_string()], true));
    for silent in ["declined@example.com", "rsvp-false@example.com"] {
        assert!(za_full_request_for(&organized, &alice(), silent).is_none(), "{silent}");
    }
}
```

- [ ] **Step 2: Write the failing integration cases**

`case_02_refusal`: `refuse_once(alice, bob, "pin-02")`.

```rust
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
assert!(new_inbox_items(test, bob.id, &before).await.is_empty());
let listing = contacts(ALICE).await;
let entry = listed(&listing, BOB);
assert_eq!((entry["withheld"].clone(), entry["source"].clone()), (json!(1), json!("first-use")));
assert_ne!(entry["served_fingerprint"], entry["fingerprint"]);
let first = withheld(test, alice.id, BOB).await[0].refusal_id;
// a second edit: one entry, a new refusal_id
let again = withheld(test, alice.id, BOB).await;
assert_eq!(again.len(), 1);
assert_ne!(again[0].refusal_id, first);
// The M48 check, in three steps:
// 1. restore bob's key with invalidation: substitute_public_key(test, bob.id, &original, true);
// 2. warm his account cache with the real key: lookup(ALICE, BOB) (200). A cold cache loads public_key
//    from the vault record (common/src/cache/principals.rs:459-471), so without this the mutant would
//    read the substituted key too;
// 3. substitute with invalidate: false and edit: a fan-out reading the cached key would deliver.
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"], "the served key is read fresh");
```

`case_04_confirm_and_redeliver`: `refuse_once(alice, bob, "pin-04")`, then bob's key restored.

```rust
assert_eq!(pin(ALICE, BOB, &display_fingerprint(&writer)).await.expect(409)["error"], "fingerprint mismatch");
let fingerprint = lookup(ALICE, BOB).await.str(200, "fingerprint");
assert_eq!(fingerprint, display_fingerprint(&served(test, bob.id).await));
let reply = pin(ALICE, BOB, &fingerprint.to_uppercase().replace(' ', "")).await.expect(200);
assert_eq!(reply, json!({"ok": true, "delivered": 1, "failed": 0, "dropped": 0, "remaining": 0}));
assert_eq!(trigger_drain(BOB, STRONG).await, 207);
assert!(pending_ids(test, bob.id).await.is_empty(), "bob's drain opens the redelivered item");
assert!(body(&bob.client, &copy_href(test, bob.id, "pin-04").await).await.contains("v2"));
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
assert_eq!(pin_record(test, alice.id, STRONG).await.unwrap().pin(BOB).unwrap().source, PinSource::Confirmed);
assert!(withheld(test, alice.id, BOB).await.is_empty());
// the batch limit: bob substituted, alice creates 33 events naming him (pin-04-1 .. pin-04-33, one PUT
// each, each refused), bob restored. If za_post's 5 s client times out on the 32-entry call, this
// sub-case uses a client with a longer timeout; REDELIVER_BATCH is never lowered.
assert_eq!(pin(ALICE, BOB, &fingerprint).await.expect(200), json!({"ok": true, "delivered": 32, "failed": 0, "dropped": 0, "remaining": 1}));
assert_eq!(pin(ALICE, BOB, &fingerprint).await.expect(200)["delivered"], 1);
```

`case_06_fail_closed`: alice holds a record pinning bob and carol; `set_raw_pins(alice, None)`; alice invites both (`pin-06`).

```rust
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
assert_eq!(attendee_statuses(&alice.client, &path, CAROL).await, ["3.8"]);
assert!(new_inbox_items(test, bob.id, &b).await.is_empty() && new_inbox_items(test, carol.id, &c).await.is_empty());
assert_eq!(raw_pins(test, alice.id).await, None, "fail closed writes no record");
let listing = contacts(ALICE).await;
assert_eq!((listing["record_missing"].clone(), listing["pins"].clone()), (json!(true), json!([])));
let reply = pin(ALICE, BOB, &lookup(ALICE, BOB).await.str(200, "fingerprint")).await.expect(200);
assert_eq!(reply["delivered"], 0, "refusals while failing closed record nothing");
assert_eq!(contacts(ALICE).await["record_missing"], false);
set_raw_pins(test, alice.id, None).await;
assert_eq!(forget(ALICE, None).await.expect(200), json!({"ok": true}));
let listing = contacts(ALICE).await;
assert_eq!((listing["record_missing"].clone(), listing["pins"].clone()), (json!(false), json!([])));
// alice's next edit sends to both, first use
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
assert_eq!(attendee_statuses(&alice.client, &path, CAROL).await, ["1.2"]);
```

`case_07_repair_keeps_fail_closed`: alice pinned, `set_raw_pins(alice, None)`, `original = substitute_public_key(alice, &writer_key(), true)`.

```rust
assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
assert_eq!(stored_key(test, alice.id).await, original, "the drain's self-check repairs");
assert_eq!(raw_pins(test, alice.id).await, None, "the repair creates no record");
substitute_public_key(test, alice.id, &writer_key(), true).await;
let listing = contacts(ALICE).await;
assert_eq!((listing["stored_key_matches"].clone(), listing["record_missing"].clone()), (json!(false), json!(true)));
assert_eq!(listing["fingerprint"], display_fingerprint(&original.clone().try_into().unwrap()));
assert_eq!(stored_key(test, alice.id).await, original, "the listing's self-check repairs");
assert_eq!(raw_pins(test, alice.id).await, None);
// alice invites bob ("pin-07")
assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
assert!(new_inbox_items(test, bob.id, &before).await.is_empty());
assert_eq!(raw_pins(test, alice.id).await, None);
```

Cleanup: `forget(ALICE, None)`.

`case_08_stale_first_use`: `za_unpin(carol)`; park `put_raw(CAROL, path, invite_from(CAROL, "pin-08", START_A, &[ALICE]))` at `PIN_DECIDE_PAUSE`; `pin(CAROL, ALICE, ..)` (200: record and rewrap); `set_raw_pins(carol, None)`; finish.

```rust
assert_eq!(parked.finish().await, 201);
assert_eq!(attendee_statuses(&carol.client, &path, ALICE).await, ["3.8"]);
assert!(new_inbox_items(test, alice.id, &before).await.is_empty());
assert_eq!(raw_pins(test, carol.id).await, None, "the stale fan-out published no record");
```

Cleanup: `za_unpin(carol)`.

`case_10_replay` (asserts limit 2, not a protection): `za_unpin(carol)`; `legacy = saved_wrap(carol)`; carol invites alice (`pin-10a`, first use); `za_post("password", &json!({"username": CAROL, "password": STRONG, "new_password": NEW}))` (200); `restore_wrap(carol, &legacy)`; `set_raw_pins(carol, None)`; carol, logging in with `NEW`, invites bob (`pin-10b`); then `raw_pins` is saved, `contacts/pin` (through `za_post` with `NEW`) confirms bob, the saved bytes are written back, `revision = vault_revision(carol)` is read, and carol makes a DAV request (`trigger_drain(CAROL, NEW)`).

```rust
// right after restore_wrap and set_raw_pins, before pin-10b (whose first-use batch rewraps)
assert_eq!(wrap_mark(test, carol.id, NEW).await, ZaMark::Legacy);
// after pin-10b
assert_eq!(new_inbox_items(test, bob.id, &before).await.len(), 1, "first use reopened");
assert!(pin_record(test, carol.id, NEW).await.unwrap().pin(BOB).is_some());
// after the Confirmed pin, the replayed record and carol's next request
assert_eq!(pin_record(test, carol.id, NEW).await.unwrap().pin(BOB).unwrap().source, PinSource::FirstUse);
assert_eq!(vault_revision(test, carol.id).await, revision, "no generation bump, no signal");
```

Cleanup: the password back to `STRONG`, `za_unpin(carol)`.

`case_14_delegates`: (organizer side) alice invites bob (`pin-14`, `event_id = document_id_of(test, alice.id, &path)`), bob drains; `pin(ALICE, CAROL, ..)`; carol's key substituted; bob delegates to carol (his line `PARTSTAT=DELEGATED;DELEGATED-TO="mailto:key3@example.com"`, plus `ATTENDEE;DELEGATED-FROM="mailto:key2@example.com";PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key3@example.com`); `trigger_drain(ALICE)`. (Attendee side) bob pins alice first; alice invites bob (`pin-14b`), bob drains; alice's key substituted; bob delegates the master to carol (refused); alice's key restored; `pin(BOB, ALICE, ..)`; `trigger_drain(ALICE)`.

```rust
assert_eq!(attendee_statuses(&alice.client, &path, CAROL).await, ["3.8"]);
let entries = withheld(test, alice.id, CAROL).await;
assert_eq!((entries.len(), entries[0].document_id), (1, event_id), "the organizer event's document");
// carol's key restored
assert_eq!(pin(ALICE, CAROL, &fingerprint).await.expect(200)["delivered"], 1);
// attendee side
let entry = &withheld(test, bob.id, ALICE).await[0];
assert!(entry.instances.master, "the refused REPLY records the master");
assert_eq!(pin(BOB, ALICE, &fingerprint).await.expect(200)["delivered"], 1);
assert_eq!(partstat_of(&alice.client, &path_b, BOB).await.as_deref(), Some("DELEGATED"));
assert!(body(&alice.client, &path_b).await.contains("DELEGATED-FROM"), "the redelivery carries bob's delegate line");
```

`case_15_pre_pin`: `za_unpin(alice)`; `pin(ALICE, CAROL, ..)` before any invitation; carol's key substituted; alice invites carol (`pin-15`).

```rust
assert_eq!(wrap_mark(test, alice.id, STRONG).await, ZaMark::Pinned, "the endpoint's first pin is the batch");
assert_eq!(attendee_statuses(&alice.client, &path, CAROL).await, ["3.8"]);
assert!(new_inbox_items(test, carol.id, &before).await.is_empty());
assert_eq!(withheld(test, alice.id, CAROL).await.len(), 1);
```

`case_17_cleanup_races`: four parts, each on its own event and its own sub-address of bob, `SUB_A` .. `SUB_D` = `key2+pin17a@example.com` .. `key2+pin17d@example.com` (sub-addresses pin and withhold separately, 4.3, and deliver to bob's account), which alice pins (`lookup`, then `pin`) before the part starts, so every `withheld` and `contacts/pin` of a part sees only its own entries. A part starts from one entry, `refuse_once_at(alice, bob, SUB_x, "pin-17x")` with bob's key restored, and ends by deleting its event and `drop_entries(ALICE, SUB_x)`.
- (a) `pin-17a`: park `pin(ALICE, SUB_A, ..)` at `REDELIVER_CLEANUP_PAUSE`; meanwhile bob's key is substituted, alice edits the event (a newer refusal) and the key is restored; finish.
- (b) `pin-17b`, freshly created and refused once, so A consumes the first `refusal_id` the entry ever had and a counter would give the recreated entry the same one: two calls of `pin(ALICE, SUB_B, ..)` read it; A parked at `REDELIVER_CLEANUP_PAUSE`; B runs to completion and retires it; a refusal recreates it; A finishes.
- (c) `pin-17c`: park alice's ordinary edit at `RETIRE_PAUSE` (`put_raw`, after `SUB_C`'s item); meanwhile bob's key is substituted, a second PUT of the event (same session) refuses `SUB_C`, and the key is restored; finish (the parked PUT's commit loses its archive assertion; its status is not asserted).
- (d) `pin-17d`: park `pin(ALICE, SUB_D, ..)` at `RETIRE_PAUSE` (its inner fan-out); meanwhile the same newer refusal as in (a); finish.

```rust
// (a) on SUB_A and (d) on SUB_D
let reply = parked.finish().await.expect(200);
assert_eq!((reply["delivered"].clone(), reply["remaining"].clone()), (json!(0), json!(1)));
let entries = withheld(test, alice.id, sub).await;
assert_eq!((entries.len(), entries[0].refusal_id), (1, newer), "the re-armed entry stays");
// (b) on SUB_B
assert_eq!(b_reply["delivered"], 1);
assert_eq!(parked.finish().await.expect(200)["remaining"], 1);
assert_eq!(withheld(test, alice.id, SUB_B).await.len(), 1, "A's retry kept the recreated entry");
// (c) on SUB_C
let _ = parked.finish().await;
let entries = withheld(test, alice.id, SUB_C).await;
assert_eq!((entries.len(), entries[0].refusal_id), (1, newer), "the newer refusal survives phase 4");
```

`case_18_event_identity`: alice holds UID `pin-18` in `default` (document `a`) and in a new calendar `pin18b` (document `b`; both read with `document_id_of`), both naming bob; bob drains. (1) bob substituted, only `b` edited to `B v2`, restored, `pin`. (2) Both edited (`A v3`, `B v3`) while substituted, restored, `pin`. (3) `b` edited while substituted, deleted with `NO_REPLY`, bob's key restored, then `b` recreated under `pin-18` (after the restore, so the create's fan-out does not refuse bob for the new document), `pin`.

```rust
// (1)
assert_eq!(withheld(test, alice.id, BOB).await.iter().map(|e| e.document_id).collect::<Vec<_>>(), [b]);
assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["delivered"], 1);
assert_eq!(opened_summaries(test, bob.id, &before).await, ["B v2"]);
// (2)
assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["delivered"], 2);
assert_eq!(opened_summaries(test, bob.id, &before2).await, ["A v3", "B v3"]);
// (3)
let reply = pin(ALICE, BOB, &fp).await.expect(200);
assert_eq!((reply["delivered"].clone(), reply["dropped"].clone()), (json!(0), json!(1)));
```

`case_19_participation_filter`: bob pins alice first; alice invites bob and carol to a daily event (`pin-19`, `RRULE:FREQ=DAILY;COUNT=5`) with an override X naming bob and carol; bob drains; alice removes bob from X (a removal CANCEL; a new override is messaged only to its own attendees, so X must exist first); bob drains (his copy holds X cancelled, naming him); alice's key substituted; bob sets the master ACCEPTED (refused); alice's key restored; `pin(BOB, ALICE, ..)`; `trigger_drain(ALICE)`.

```rust
assert_eq!(pin(BOB, ALICE, &fp).await.expect(200)["delivered"], 1);
assert_eq!(partstat_of(&alice.client, &path, BOB).await.as_deref(), Some("ACCEPTED"), "a whole-copy REPLY would be rejected: OutOfSequence (the reconciled X is ahead of bob's), or SenderIsNotParticipant without the reconciliation");
assert_eq!(vevent_count(&alice.client, &path).await, vevents_before, "alice gains no override");
```

`case_20_drain_before_build`: bob pins alice first; alice invites bob to a daily event with an existing override X naming him (`pin-20`); bob drains; alice's key substituted; bob sets the master ACCEPTED (refused); alice's key restored; alice moves X (a partial REQUEST, pending in bob's inbox); no request as bob.

```rust
assert_eq!(pending_ids(test, bob.id).await.len(), 1);
assert_eq!(pin(BOB, ALICE, &fp).await.expect(200)["delivered"], 1);
assert!(pending_ids(test, bob.id).await.is_empty(), "contacts/pin drained before building");
assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
assert_eq!(partstat_of(&alice.client, &path, BOB).await.as_deref(), Some("ACCEPTED"));
```

`rf_1_address_forms` (Review Focus 1), alice inviting `Key2@Example.COM`:

```rust
for bad in ["mailto:key2@example.com", "Key2 <key2@example.com>"] {
    assert_eq!(lookup(ALICE, bad).await.status, 400, "{bad}");
}
assert_eq!(lookup(ALICE, " Key2@Example.COM ").await.expect(200)["address"], BOB);
for other in ["plain@example.com", "nobody@example.com"] {
    assert_eq!(lookup(ALICE, other).await.expect(409)["error"], "not a key account");
}
assert_eq!(lookup(ALICE, ALICE).await.expect(200)["fingerprint"], display_fingerprint(&served(test, alice.id).await));
let listing = contacts(ALICE).await;
let bobs: Vec<&str> = listing["pins"].as_array().unwrap().iter().map(|p| p["address"].as_str().unwrap()).filter(|a| a.eq_ignore_ascii_case(BOB)).collect();
assert_eq!(bobs, ["key2@example.com"], "keyed exactly by the normalized address");
let fp = lookup(ALICE, BOB).await.str(200, "fingerprint");
pin(ALICE, " Key2@Example.COM ", &fp).await.expect(200);
let record = pin_record(test, alice.id, STRONG).await.unwrap();
assert_eq!(record.pins.iter().filter(|p| p.address.eq_ignore_ascii_case(BOB)).count(), 1, "one pin after the page's form too");
```

`rf_4_moved_source` (Review Focus 4): `refuse_once(alice, bob, "pin-rf4")`; `document_id = document_id_of(test, alice.id, &path)`; MKCALENDAR `pinrf4`; MOVE the event there; bob's key restored.

```rust
assert_eq!(withheld(test, alice.id, BOB).await[0].document_id, document_id, "a MOVE keeps the id");
assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["delivered"], 1);
```

`rf_5_never_pinned` (Review Focus 5):

```rust
za_unpin(test, carol.id, STRONG).await;
assert_eq!(contacts(CAROL).await, json!({
    "fingerprint": display_fingerprint(&served(test, carol.id).await), "stored_key_matches": true,
    "key_repaired_at": null, "record_missing": false, "pins": [] }));
assert_eq!(forget(CAROL, Some(BOB)).await.expect(200), json!({"ok": true}));
assert_eq!(forget(CAROL, None).await.expect(200), json!({"ok": true}));
assert_eq!(raw_pins(test, carol.id).await, None, "nothing here creates a record");
assert_eq!(wrap_mark(test, carol.id, STRONG).await, ZaMark::Legacy);
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p groupware`, then the za suite.
Expected: compile errors naming `za_participation_reply`; then, with stubs, 404 from `/api/vault/contacts`.

- [ ] **Step 4: Implement `pinning.rs` (groupware), the hook and `contacts.rs`**

`contacts.rs` starts with the SPDX header and a module doc citing spec 6. Bodies and responses go through `json`/`conflict`, so they stay untraced and `no-store`; the route sits behind `handle_vault_request`'s anonymous-request rate limit, and the CORS headers come from `za_is_vault_path` (`crates/http/src/request.rs:552-571`) with no change.

- [ ] **Step 5: Run everything this task touches**

Run: `cargo test -p groupware`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite.
Expected: pass; `clean`.

- [ ] **Step 6: Commit**

```bash
git add crates/http/src/api/contacts.rs crates/http/src/api/mod.rs crates/http/src/api/vault.rs crates/groupware/src/calendar/pinning.rs crates/groupware/src/calendar/mod.rs crates/common/src/za/test_hooks.rs tests/src/za/pinning.rs
git commit -m "Key pinning: the contacts endpoints, redelivery of withheld changes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 6: SEQUENCE floor and reconciliation

**Files:**
- Create: `crates/groupware/src/calendar/sequence.rs`
- Modify: `crates/groupware/src/calendar/mod.rs:7-15` (`pub mod sequence;`)
- Modify: `crates/groupware/src/calendar/fanout.rs` (the reconciliation before phase 1; `ZaFanOutResult.sequences`)
- Modify: `crates/dav/src/calendar/update.rs:114` (`let mut ical`), `:232-240` (the floor between `let old_ical = ..` and `CalendarEventData::new_for`), `:360` and `:573` (`za_set_sequences` after `za_set_schedule_status`)
- Modify: `crates/groupware/src/calendar/drain.rs:1108` (`za_set_sequences` after `za_set_schedule_status`)
- Modify: `crates/http/src/api/contacts.rs` (step 4 applies `za_set_sequences`)
- Modify: `tests/src/za/pinning.rs` (case 12)

**Interfaces:**
- Consumes: `drain::{InstanceKey, instance_key}` (Task 4); the fan-out phases (Task 4); `itip_snapshot`; `ICalendar::build_tz_resolver`.
- Produces, in `groupware::calendar::sequence`:
  - `pub fn za_instance_keys(ical: &ICalendar) -> Vec<(u16, InstanceKey)>`: every scheduling component's id and its `InstanceKey`, the RECURRENCE-ID resolved to a UTC instant with the tree's own VTIMEZONEs and the `RANGE=THISANDFUTURE` flag, exactly as `itip_snapshot` computes `RecurrenceId::date` (`snapshot.rs:251-280`), without needing an ORGANIZER.
  - `pub fn za_sequence_floor(new: &mut ICalendar, stored: &ICalendar, addresses: &[String]) -> bool`: only when the stored tree's ORGANIZER is one of `addresses`; per instance of `new` also in `stored`, a lower or absent SEQUENCE becomes the stored value; a higher one stays; no component is added or removed. `true` when anything changed.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ZaSequence { pub instance: InstanceKey, pub sequence: i64 }`.
  - `pub fn za_reconcile_sequences(committed: &mut ICalendar, messages: &mut [ItipMessage<ICalendar>], addresses: &[String]) -> Vec<ZaSequence>`: over every instance the organizer REQUESTs and CANCELs (`from_organizer`, METHOD REQUEST or CANCEL) carry, the full REQUEST included, matched by `InstanceKey` from each message's own tree, sets the committed component and every message's component to the highest SEQUENCE among them; an instance the committed tree lacks sets only the messages; REPLYs are untouched; nothing when `committed` has no snapshot with a local ORGANIZER (`itip_cancel`'s fallback). Returns the committed values it set.
  - `pub fn za_set_sequences(ical: &mut ICalendar, sequences: &[ZaSequence])`.
- Produces, in `fanout`: `ZaFanOutResult` gains `pub sequences: Vec<ZaSequence>`; `za_fan_out` reconciles a clone of `event` and `messages` before phase 1 unless `kind == Deletion`, and later phases (Task 7's full state) build from that reconciled clone. Every caller applies `za_set_sequences` to its tree before sealing, next to `za_set_schedule_status` (the update and create PUTs, the delegating merge, `contacts/pin` step 4).

The floor runs on every PUT of an existing event of a key account (`za_keys.is_some()`), whether or not it builds messages, before upstream's scheduling step (5.1).

- [ ] **Step 1: Write the failing unit tests in `sequence.rs`**

The reconciliation fixture: a committed tree (VTIMEZONE `Fixed-Plus-One`, one STANDARD at `+0100`) with the master at 3, override X (`RECURRENCE-ID:20990303T100000Z`) at 6 (stored 4, the client bumped by two) and Z (`20990305T100000Z`) at 2; messages: [0] a partial REQUEST for Z at 3; [1] a CANCEL built from the old tree, whose components are ordered W, X (W an override removed in the same PUT), X written `RECURRENCE-ID;TZID=Fixed-Plus-One:20990303T110000` at 5; [2] a CANCEL for the removed W (`20990301T100000Z`) at 2; [3] bob's REPLY for X at 0.

```rust
#[test]
fn the_reconciliation_takes_each_instances_maximum_with_the_committed_component() {
    let (mut committed, mut messages) = fixture();
    let before = committed.components.len();
    let mut set = za_reconcile_sequences(&mut committed, &mut messages, &alice());
    set.sort_by_key(|s| s.instance);
    assert_eq!(seq(&committed, X), 6, "the committed component counts");
    assert_eq!(seq(&messages[1].message, X), 6, "matched by instant and instance, not text or component id");
    assert_eq!((seq(&committed, Z), seq(&messages[0].message, Z)), (3, 3));
    assert_eq!(seq(&messages[2].message, W), 2, "an instance the committed tree lacks");
    assert_eq!(committed.components.len(), before, "no component is added");
    assert_eq!(seq(&messages[3].message, X), 0, "replies are untouched");
    assert_eq!(set, [ZaSequence { instance: X, sequence: 6 }, ZaSequence { instance: Z, sequence: 3 }]);
    let (mut applied, _) = fixture();
    za_set_sequences(&mut applied, &set);
    assert_eq!(applied, committed);
}

#[test]
fn the_floor_raises_lower_values_and_keeps_higher_ones() {
    // stored: master 4, X 9 (TZID form), Y 5, R 1; new: master 6, X 7 (UTC form), Y without SEQUENCE,
    // a new override Q at 0, R removed.
    assert!(za_sequence_floor(&mut new, &stored, &alice()));
    assert_eq!((seq(&new, MASTER), seq(&new, X), seq(&new, Y), seq(&new, Q)), (6, 9, 5, 0));
    assert!(component(&new, R).is_none(), "a removed component is not re-added");
    assert_eq!(new.components.len(), before);
}

#[test]
fn the_floor_applies_only_to_a_local_organizer() {
    assert!(!za_sequence_floor(&mut new, &stored_by_someone_else, &alice()));
    assert_eq!(new, untouched);
}
```

- [ ] **Step 2: Write case 12 (integration)**

On a daily event of alice (`RRULE:FREQ=DAILY;COUNT=5`) with existing overrides A (day 2) and B (day 3) naming bob and carol, bob drained, each variant its own event:
- `pin-12a`, two refused partials: bob substituted; alice moves A, then moves B; bob restored; `pin`; bob drains.
- `pin-12b`, a delivered partial: alice moves A (delivered); bob drains; bob substituted; alice moves B; restored; `pin`; bob drains.
- `pin-12c`, lowered: as `pin-12b`, but the second PUT also writes A's SEQUENCE one below the stored value.

```rust
assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["delivered"], 1);
assert_eq!(trigger_drain(BOB, STRONG).await, 207);
assert!(pending_ids(test, bob.id).await.is_empty());
let copy = body(&bob.client, &copy_href(test, bob.id, uid).await).await;
assert!(copy.contains(A_MOVED) && copy.contains(B_MOVED), "{uid}: {copy}"); // pin-12a
assert!(copy.contains(B_MOVED), "{uid}: no OutOfSequence: {copy}");         // pin-12b, pin-12c
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p groupware sequence`, then the za suite.
Expected: compile errors naming `za_reconcile_sequences`; then case 12's `pin-12b` fails at "no OutOfSequence".

- [ ] **Step 4: Implement `sequence.rs`, the fan-out pass and the call sites**

- [ ] **Step 5: The open item 4 probe (throwaway)**

Add a temporary test to `sequence.rs`: an organizer's old tree with a daily master naming bob and carol and an override R3 (`RECURRENCE-ID:20990303T100000Z`) naming both; the new tree keeps the master, removes R3 and adds R2 (`RECURRENCE-ID:20990302T100000Z`, `STATUS:CANCELLED`, bob at NEEDS-ACTION), its RECURRENCE-ID at the same property position as R3's (so R2 sorts first: `RecurrenceId`'s derived `Ord` compares `entry_id` before `date`); call `itip_update(&mut new, &old, &["org@example.com".into()])`.
Run: `cargo test -p groupware probe_partial_cancel_tree_switch -- --nocapture`.
Record in the task report whether it panics, and where (expected: `organizer.rs:243`, `Option::unwrap()` on `None`). Then delete the test; `git status --short crates/` shows only this task's files. No fix.

- [ ] **Step 6: Run everything this task touches**

Run: `cargo test -p groupware`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite; `ZA_KEY_ACCOUNTS=1` `webdav_tests`.
Expected: pass; `clean`.

- [ ] **Step 7: Commit**

```bash
git add crates/groupware/src/calendar/sequence.rs crates/groupware/src/calendar/mod.rs crates/groupware/src/calendar/fanout.rs crates/groupware/src/calendar/drain.rs crates/dav/src/calendar/update.rs crates/http/src/api/contacts.rs tests/src/za/pinning.rs
git commit -m "Key pinning: the SEQUENCE floor and the fan-out's reconciliation" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 7: The two-item rule on ordinary delivery

**Files:**
- Modify: `crates/groupware/src/calendar/fanout.rs` (phase 3's full state and skips; the attendee DELETE's DECLINED; phase 4 for a deletion)
- Modify: `crates/groupware/src/calendar/pinning.rs` (`za_decline_all`; unit test)
- Modify: `crates/groupware/src/calendar/storage.rs:555-604` (`delete_all_za` builds a `ZaDeleteMessage` whenever the scheduling gate holds)
- Modify: `crates/dav/src/calendar/delete.rs:252-264`
- Modify: `tests/src/za/pinning.rs` (cases 5, 11, 13, 21)

**Interfaces:**
- Consumes: `za_full_request_for`, `za_participation_reply` (Task 5); the reconciled committed clone (Task 6); the phases and `ZaFanOutKind` (Task 4); `itip_snapshot`, `itip_build_envelope`, `itip_add_tz`.
- Produces:
  - `ZaDeleteMessage.message: Option<ItipMessage<ICalendar>>`: `delete_all_za` builds the struct under `za_keys` whenever `send_itip`, a schedule tag and a future end hold, with `itip_cancel`'s `Ok` or `None` (5.1: the attendee's DELETE enters the fan-out even when `itip_cancel` returns nothing); `delete.rs` passes `message.message.into_iter().collect()`.
  - `pub fn za_decline_all(ical: &ICalendar, addresses: &[String]) -> Option<ItipMessage<ICalendar>>` (`pinning`): `itip_cancel`'s attendee branch (`event_cancel.rs:82-121`, `is_deletion` true) over a fork copy of `attendee_decline` whose local-attendee gate is `attendee.is_server_scheduling && attendee.rsvp.is_none_or(|rsvp| rsvp)`, without the PARTSTAT match of `attendee.rs:301-308`.
  - In `pinning.rs` (tests): `opened(test, account_id, before: &[u32]) -> Vec<ICalendar>` (the new items, opened, in document-id order); `fn methods(items: &[ICalendar]) -> Vec<Option<ICalendarMethod>>`; `fn sequence_of(ical: &ICalendar, recurrence_id: &str) -> Option<i64>` (the component whose RECURRENCE-ID text is that UTC value); `partstat_in(client, href, recurrence_id: Option<&str>, address) -> Option<String>`.

The rule (5.1, 5.2 phases 3 and 4), which the signatures do not determine:

```text
kind == Deletion, the sender an attendee, no REPLY among messages: resolve the ORGANIZER as phase 1 resolves a
  recipient; when it is internal, read the pin state, and when that holds an entry for (source, the ORGANIZER's
  normalized address), push za_decline_all(event, addresses) and route it with the rest (the stored pair's token)
kind == Deletion with a message to send: phase 1 keeps the refusal_id of every entry of (source.document_id, source.uid)
Phase 3, kind == Change, per Deliver recipient with an entry E for (source, address):
  own = the recipient's upstream messages, upstream's order (the full REQUEST before a partial)
  no full state when: an own message is a REQUEST or a CANCEL with a component without RECURRENCE-ID;
    or the relationship ended (organizer: the reconciled committed tree names no ATTENDEE with this address);
    or it yields nothing
  full = organizer: za_full_request_for(committed, addresses, address)
         attendee:  za_participation_reply(committed, addresses, &E.instances, the own REPLY's from)
  write full (organizer: union.only(address); attendee: the replier's token), then own;
  a failed full (5.1) never stops own; E is retired in phase 4 only when full, if any, and own were all written
Phase 4, kind == Deletion with a message to send (upstream's, or the fork's DECLINED):
  retire every entry of (source.document_id, source.uid) under the refusal_id phase 1 read, items written or not
```

- [ ] **Step 1: Write the failing unit test in `pinning.rs` (groupware)**

```rust
#[test]
fn the_fork_decline_ignores_partstat() {
    // bob's copy with bob DECLINED and no SCHEDULE-FORCE-SEND
    assert!(itip_cancel(&declined, &bob(), true).is_err(), "upstream builds nothing");
    let message = za_decline_all(&declined, &bob()).expect("the fork declines");
    assert_eq!((message.from.as_str(), message.from_organizer), (BOB, false));
    assert!(message.to.contains(&ALICE.to_string()));
    assert_eq!(reply_partstat(&message, None, BOB).as_deref(), Some("DECLINED"));
    for silent in [&rsvp_false, &client_scheduled] {
        assert!(za_decline_all(silent, &bob()).is_none(), "RSVP and SCHEDULE-AGENT still gate");
    }
}
```

- [ ] **Step 2: Write the failing integration cases**

`case_05_forget_keeps_entries`: `refuse_once(alice, bob, "pin-05")`; bob restored; `forget(ALICE, Some(BOB))`; alice edits SUMMARY.

```rust
assert!(pin_record(test, alice.id, STRONG).await.unwrap().pin(BOB).is_none());
assert_eq!(withheld(test, alice.id, BOB).await.len(), 1, "forget keeps the entry");
// after the edit
assert_eq!(pin_record(test, alice.id, STRONG).await.unwrap().pin(BOB).unwrap().source, PinSource::FirstUse);
assert_eq!(opened(test, bob.id, &before).await.len(), 1, "a master REQUEST: no separate full state");
assert!(withheld(test, alice.id, BOB).await.is_empty(), "drained under the two-item rule");
```

`case_11_ordinary_delivery`; each sub-case that counts items starts from a fresh refusal:
- Reply (`pin-11r`): bob pins alice first; alice's key substituted; bob DECLINED (refused); restored; bob ACCEPTED.
- Organizer (`pin-11o`, daily, override A naming bob): bob substituted; alice moves A (refused); restored; alice adds a STATUS:CANCELLED override B naming bob at NEEDS-ACTION. Then a fresh refusal and a master change.
- Removal (`pin-11x`, daily, override X naming bob and carol, bob on the master): an older update (DESCRIPTION) refused; restored; alice removes bob from X (carol stays). Repeated as `pin-11x2` with the client bumping X's SEQUENCE in that PUT.
- Mixed (`pin-11m`): bob on overrides X and Z but not the master, carol on X, bob holding an entry; one PUT adds `dave@external.test` to the master and removes bob from X, the client bumping X.
- Master removal (`pin-11y`): bob the master's only attendee besides alice, also on override Y; a fresh refusal; alice removes bob from the master.
- DELETE (`pin-11d`): bob pins alice first; alice's key substituted; bob ACCEPTED (refused); restored; bob deletes his copy. Already DECLINED (`pin-11d2`): the same with bob DECLINED as the refused change.
- Withheld, then DECLINED (`pin-11w`): bob substituted; alice edits SUMMARY (no SEQUENCE bump; an entry for bob); restored; bob DECLINED (delivered; alice drains); alice edits SUMMARY again; then `pin(ALICE, BOB)`. "No item for him" is upstream's behaviour (a DECLINED attendee gets no update, and no fork mutant reaches it); `dropped 1` is the fork's check.

```rust
// Reply
let items = opened(test, alice.id, &before).await;
assert_eq!(items.len(), 2, "his current participation, then the REPLY");
assert!(items.iter().all(|item| item_partstat(item, BOB).as_deref() == Some("ACCEPTED")));
assert_eq!(pin(BOB, ALICE, &fp).await.expect(200), json!({"ok": true, "delivered": 0, "failed": 0, "dropped": 0, "remaining": 0}));
assert_eq!(partstat_of(&alice.client, &path, BOB).await.as_deref(), Some("ACCEPTED")); // after alice's drain
// Organizer
assert_eq!(methods(&opened(test, bob.id, &before).await), [Some(ICalendarMethod::Request), Some(ICalendarMethod::Cancel)]);
assert!(withheld(test, alice.id, BOB).await.is_empty(), "retired after both");
assert!(copy.contains(A_MOVED) && copy.contains("STATUS:CANCELLED"), "A updated, B cancelled"); // bob's copy after his drain
assert_eq!(opened(test, bob.id, &before_master).await.len(), 1, "a master change gives one item");
// Removal (pin-11x and pin-11x2)
assert_eq!(methods(&items), [Some(ICalendarMethod::Request), Some(ICalendarMethod::Cancel)]);
assert!(copy.contains(DESCRIPTION_V2), "bob keeps the update");
assert_eq!(partstat_in(&bob.client, &copy_href, Some(X), BOB).await, None, "X cancelled for bob");
// Mixed
assert_eq!(methods(&items), [Some(ICalendarMethod::Request), Some(ICalendarMethod::Cancel)], "no extra full state");
assert_eq!(sequence_of(&items[1], X), sequence_of(&items[0], X), "the CANCEL raised to the full REQUEST's value");
// Master removal
assert_eq!(opened(test, bob.id, &before).await.len(), 1);
// DELETE and already DECLINED
let items = opened(test, alice.id, &before).await;
assert_eq!((items.len(), item_partstat(&items[0], BOB).as_deref()), (1, Some("DECLINED")), "no full state: the source is gone");
assert!(withheld(test, bob.id, ALICE).await.is_empty(), "retired after the decline");
// Withheld, then DECLINED
assert!(opened(test, bob.id, &before_edit).await.is_empty(), "upstream sends a DECLINED attendee nothing");
assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["dropped"], 1);
```

For X "cancelled for bob", `partstat_in` returns `None` when bob's copy holds X as STATUS:CANCELLED; the implementer reads X's STATUS in the same helper if that is cleaner, asserting `STATUS:CANCELLED` on X.

`case_13_stated_losses`, one sub-test each (limit 6):
- `pin-13a`: bob substituted; alice edits (an entry); alice deletes the event with scheduling (its CANCEL refused); restored.
- `pin-13b`: bob substituted; alice removes bob from the event (CANCEL refused, an entry); restored; `pin`.
- `pin-13c`: bob pins alice first; daily, override X naming bob; alice's key substituted; bob deletes occurrence X (a refused decline recording X); restored; `pin(BOB, ALICE)`; alice drains.
- `pin-13d`: bob pins alice first; alice's key substituted; bob delegates the master to carol (refused); restored; alice changes the master (a full REQUEST); bob drains; `pin(BOB, ALICE)`; alice drains.

```rust
// pin-13a
assert!(withheld(test, alice.id, BOB).await.is_empty(), "the deletion retires the document's entries");
assert!(!body(&bob.client, &copy).await.contains("STATUS:CANCELLED"), "bob keeps his copy");
// pin-13b
assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["dropped"], 1);
assert!(!body(&bob.client, &copy).await.contains("STATUS:CANCELLED"));
// pin-13c
assert_eq!(pin(BOB, ALICE, &fp).await.expect(200)["dropped"], 1, "X no longer exists in bob's copy");
assert!(!body(&alice.client, &path).await.contains("PARTSTAT=DECLINED"), "the decline is not reconstructed");
// pin-13d
assert!(!body(&alice.client, &path).await.contains(CAROL), "alice gains no delegate");
```

`case_21_recorded_instances`: bob pins alice first (in each run); a daily event (`pin-21`) naming bob on the master; bob delegates the master to carol; carol accepts (bob does not see it); alice adds override X naming only bob at NEEDS-ACTION; bob drains; alice's key substituted; bob ACCEPTED on X (refused); restored. Run 1: `pin(BOB, ALICE)`, alice drains. Run 2 (`pin-21b`, the same setup): bob sets X TENTATIVE (ordinary recovery), alice drains. Genuine delegation (`pin-21c`): alice's key substituted; bob delegates the master to carol (refused); restored; `pin(BOB, ALICE)`; alice drains.

```rust
// run 1
assert_eq!(partstat_in(&alice.client, &path, Some(X), BOB).await.as_deref(), Some("ACCEPTED"));
assert_eq!(partstat_in(&alice.client, &path, None, CAROL).await.as_deref(), Some("ACCEPTED"), "the master is not exported");
// run 2
assert_eq!(partstat_in(&alice.client, &path_b, Some(X), BOB).await.as_deref(), Some("TENTATIVE"));
assert_eq!(partstat_in(&alice.client, &path_b, None, CAROL).await.as_deref(), Some("ACCEPTED"));
// genuine delegation
assert_eq!(partstat_in(&alice.client, &path_c, None, CAROL).await.as_deref(), Some("NEEDS-ACTION"), "alice gains carol");
assert_eq!(opened(test, carol.id, &carol_before).await.len(), 1, "and invites her");
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p groupware`, then the za suite.
Expected: compile error naming `za_decline_all`; then case 5 or 11 fails on its item counts.

- [ ] **Step 4: Implement the rule, the deletion path and `za_decline_all`**

- [ ] **Step 5: Run everything this task touches**

Run: `cargo test -p groupware`; the formatting check; the per-task compile check; `cargo check -p tests --tests`; the za suite; `ZA_KEY_ACCOUNTS=1` `webdav_tests`.
Expected: pass; `clean`.

- [ ] **Step 6: Commit**

```bash
git add crates/groupware/src/calendar/fanout.rs crates/groupware/src/calendar/pinning.rs crates/groupware/src/calendar/storage.rs crates/dav/src/calendar/delete.rs tests/src/za/pinning.rs
git commit -m "Key pinning: the full state before upstream's messages, the deletion's retirement" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 8: Coverage, the contract, mutation runs and the outcome note

**Files:**
- Modify: `tests/src/za/leak.rs` (`EXTRA_CANARIES` gains `"pinaddr-canary"`; new `pub async fn test_pins`)
- Modify: `tests/src/za/mod.rs` (`leak::test_pins(&mut test).await;` after `leak::test_drain`)
- Modify: `tests/src/webdav/za_variants.rs` (`scheduling`: one assertion that a pin record exists before its `assert_is_empty`)
- Modify: `docs/zero-access/manual-checklist.md` (item 15)
- Modify: `/Users/jay/Development/circulo-account/docs/api.md` (on a new branch `contacts-api` there)
- Modify: `CLAUDE.md`
- Create: `docs/superpowers/plans/2026-10-17-zero-access-plan12-outcome.md`

**Interfaces:**
- Consumes: everything above; each task's report (kill lines, the Task 6 probe result, adjusted assertions).
- Produces: the note later plans read; api.md as the `#contacts` screen's plan will read it.

- [ ] **Step 1: The leak scan with a canary-bearing pin record**

A key account's own address is in the registry in the clear, so the canary rides in a sub-address, which pins separately (4.3). `leak::test_pins`, under `DrainOn`: alice invites `key2+pinaddr-canary@example.com` (`pin-leak`, a `FirstUse` pin); key2's key substituted with invalidation; alice edits SUMMARY (an entry for that address); then:

```rust
let record = pin_record(test, key1_id, STRONG).await.expect("the record");
assert!(record.pin("key2+pinaddr-canary@example.com").is_some() && record.withheld.len() >= 1, "the positive control");
for account_id in [key1_id, key2_id] {
    let found = scan(test, account_id).await;
    assert!(found.violations.is_empty(), "pin record leaks:\n{}", found.violations.join("\n"));
}
```

Cleanup: key2's key restored; the event deleted with `NO_REPLY`; key2's new items deleted; `pin` for the sub-address (drops the entry), then `forget` it.

- [ ] **Step 2: Key-mode `webdav_tests` with pin records present**

In `za_variants::scheduling`, after its first delivered invitation: `assert!(raw_pins(test, organizer_id).await.is_some(), "a first-use pin record exists when assert_is_empty runs");`.
Run: `ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture`.
Expected: pass (Task 2's exemption holds while the accounts exist).

- [ ] **Step 3: Manual checklist item 15 (open item 6)**

Append to `docs/zero-access/manual-checklist.md`:

```
15. Key pinning's refusal (`SCHEDULE-STATUS` 3.8): with clients connected to two key accounts, invite the second account from the first, then give the second account a new key (destroy it, re-create it with the same address and set it up again; only a fresh setup changes a key), and edit the event from the first client. Record how Apple Calendar, Thunderbird and DAVx5 show `3.8` on that attendee (shown as such, shown as a delivery failure, or not shown) and their versions. The account page's contacts screen is where the user compares and confirms the new key.
```

- [ ] **Step 4: api.md in `circulo-account`**

In `/Users/jay/Development/circulo-account`: `git switch -c contacts-api main`. Edit `docs/api.md`:
- The opening paragraph's list of what the API lets a person do gains "compare, confirm and forget the keys of the key-account contacts they invite".
- "Password-verified endpoints." lists `contacts`, `contacts/lookup`, `contacts/pin` and `contacts/forget` too; "Shared status codes for these five endpoints" becomes "these nine endpoints"; the `vault record missing` row adds "or the account's private key does not open".
- Four sections after `totp`, `POST /api/vault/contacts`, `/contacts/lookup`, `/contacts/pin`, `/contacts/forget`: purpose, request, success object and code table, transcribed from spec 6.2-6.5 (field names, types, meanings, every 409 string); the fingerprint format of 3.1 (with its example) in the `contacts` section; a note that addresses are normalized (lowercased, IDNA) and that `mailto:` or display-name forms are a 400; `contacts/pin`'s counts and the meaning of each outcome (6.4's outcome table, in the page's terms).
- "Flows" gains the three of 6.6 (check or pre-pin; key changed; `record_missing` recovery, `contacts/forget` with null last, finish re-pinning before editing) and the losses of a key change in the page's words (2, limit 6: after a real key change the contact's old copy no longer exists).
- "What the page never sees" is unchanged (6.6).

Commit there; no push, no pull request:

```bash
git -C /Users/jay/Development/circulo-account add docs/api.md
git -C /Users/jay/Development/circulo-account commit -m "Contract: the contacts endpoints (key pinning)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

- [ ] **Step 5: `CLAUDE.md`**

- Status: after release 2's paragraph, "Status, key pinning (spec `docs/superpowers/specs/2026-10-17-zero-access-key-pinning-design.md`, revision 11): plan 12 (`docs/superpowers/plans/2026-10-17-zero-access-12-key-pinning.md`, outcome note `docs/superpowers/plans/2026-10-17-zero-access-plan12-outcome.md`)." The controller adds the merge commit and pull request at merge time.
- "Where the code lives": `crates/vault/src/pins.rs` (the pin record) and `keys.rs` (fingerprints, `WrapMark`); `crates/common/src/auth/vault.rs` gains `za_vault_commit`, `za_pin_transact`, `za_self_check`, `za_served_public_key` and `PrincipalField::ZeroAccessPins = 151`; `crates/http/src/api/contacts.rs` (the four contacts endpoints, so "the eight `/api/vault/*` endpoints" becomes twelve); `crates/groupware/src/calendar/pinning.rs` (full state, participation REPLY, fork DECLINED) and `sequence.rs` (floor, reconciliation); the fan-out's four phases and `ZaFanOutKind`.
- Invariants: one bullet, "Key pinning (its spec section 10, 16-23): pins compared before any item, keyed by normalized address against the freshly read served key; the pinned wrap never reverts; only `contacts/pin` and `contacts/forget` with null create a record under a pinned wrap."
- The test-harness hooks list gains `PIN_DECIDE_PAUSE`, `RETIRE_PAUSE`, `REDELIVER_CLEANUP_PAUSE`, `PIN_WRITE_FAIL_ONCE`; the za suite paragraph names `tests/src/za/pinning.rs` (after the drain modules) and `leak::test_pins`; `assert_is_empty` exempts fields 150 and 151 while the account exists.
- Merge hotspots gain `crates/dav/src/calendar/update.rs` (the floor, the create's document id before the fan-out), `delete.rs`, `crates/groupware/src/calendar/storage.rs` (`ZaDeleteMessage`), `crates/http/src/api/mod.rs`, and upstream's `organizer_handle_update` partial branch, whose behaviour `sequence.rs` compensates (a change there needs a re-read).

- [ ] **Step 6: Mutation runs**

Against the branch head commit, one at a time (Global Constraints). Record the first failing assertion (`file:line`, message, left/right) of each:

| Id | Check | Edit | Expected catcher |
|----|-------|------|------------------|
| M47 | Key equality (5.1), `fanout.rs` decide | `pin == served` becomes `true` | case 2: `["3.8"]` |
| M48 | The fresh served key (5.1), `resolve_inner` | the account cache's `za_public_key` | case 2, uninvalidated: `["3.8"]` |
| M49 | Pin before deliver (5.2) | phase 2's write after phase 3 | case 16: "no item without its pin" |
| M50 | The batch's vault assertion (4.4), `za_vault_commit` | the vault `assert_value` dropped when `pins` is `Some` | case 8: "the stale fan-out published no record" |
| M51 | Entries bound to the document (4.1), `ZaPinRecord::{refuse, entry}` | `document_id` left out of the identity | case 18 (2): `delivered == 2` |
| M52 | Random `refusal_id` (4.1), `refuse` | the entry's previous id + 1, a new entry 1 | case 17 (b): "A's retry kept the recreated entry" |
| M53 | The id check (5.2 phase 4) | phase 4's `retire` ignores `consumed` | case 17 (c): "the newer refusal survives phase 4" |
| M54 | Pinned-then-legacy reads (4.4), `unwrap_private_key` | the pinned purpose not tried | case 6: `contacts` answers 409 |
| M55 | Fail closed (4.4), decide | an absent record is always first use | case 6: `["3.8"]` |
| M56 | The self-check creates no record (22) | the repair writes `ZaPinRecord::new()` when none is usable | case 7: "the repair creates no record" |
| M57 | The heal (4.4), `za_pin_transact` | a legacy wrap gets a pin-only write | case 9 (a): `ZaMark::Pinned` |
| M58 | The full state (5.1) | phase 3 writes none | case 11 Removal: `[Request, Cancel]` |
| M59 | Upstream's messages (5.1) | own messages skipped after a full state | case 11 Removal: `[Request, Cancel]` |
| M60 | The full state first (5.1) | the full state after own | case 11 Removal: `[Request, Cancel]` |
| M61 | Messages reconciled (5.1), `za_reconcile_sequences` | only the committed component set | case 11 `pin-11x2` and Mixed: "the CANCEL raised to the full REQUEST's value" |
| M62 | The committed component in the max | left out of the max | unit: "the committed component counts" |
| M63 | The floor keeps higher values | every value set to the stored one | unit `the_floor_raises_lower_values_and_keeps_higher_ones` (master 6) |
| M64 | The floor adds nothing | stored components the new tree lacks copied back | unit: "a removed component is not re-added" |
| M65 | Redelivery sends the full state (6.4) | `za_full_request_for` keeps only the last override | case 12 `pin-12a`: A and B moved |
| M66 | The reconciliation (5.1) | `za_reconcile_sequences` returns at once | case 12 `pin-12b`: "no OutOfSequence" |
| M67 | The floor (5.1), `update.rs` | the call removed | case 12 `pin-12c`: "no OutOfSequence" |
| M68 | The master-REQUEST skip (5.1) | removed | case 11: "a master change gives one item"; Mixed: "no extra full state" |
| M69 | The master-CANCEL skip (5.1) | removed | case 11 Master removal: one item |
| M70 | The participation filter (6.4) | `za_participation_reply` exports every scheduling component | case 19: `ACCEPTED` (the whole REPLY rejected: `OutOfSequence`, the reconciled X ahead of bob's; `SenderIsNotParticipant` without the reconciliation) |
| M71 | The drain before the build (6.4) | `contacts/pin`'s drain loop removed | case 20: "contacts/pin drained before building" |
| M72 | Recorded instances only (6.4) | every entry treated as `all` | case 21 run 1: "the master is not exported" |
| M73 | The fingerprint check (6.4) | the comparison removed | case 4: "fingerprint mismatch" |

M62-M64 are unit runs (`cargo test -p groupware sequence`); the rest run the za suite. The first failing assertion kills a mutation; record it, whether or not it is the catcher named (cases run in order, so an earlier case may fail first).

- [ ] **Step 7: Full verification on the branch head**

In order, one at a time: the formatting check; the per-task compile check; `cargo check -p tests --tests`; `cargo test -p vault`; `cargo test -p groupware`; `cargo test -p common auth::vault`; `cargo test -p common za::`; the za suite; the za suite with `BLOB_STORE=FileSystem`; `webdav_tests` plain; `webdav_tests` with `ZA_KEY_ACCOUNTS=1`; the product build `cargo build --release -p stalwart --no-default-features --features rocks` (in the background, no other cargo command alongside; record its time and that the fork's files produced no warning).

- [ ] **Step 8: The outcome note**

Create `docs/superpowers/plans/2026-10-17-zero-access-plan12-outcome.md` in the shape of the plan 11 note:
- **Where things stand**: the branch, every commit by task, each suite's result and time from Step 7.
- **What changed and why**: one paragraph per task, naming the spec sections.
- **Spec 13, the open items**: the six resolutions of this plan's header. Item 4: the post-pass is the plan's choice under spec 13's preamble (the recommendation stands; Jay can override it at plan review), with the cost argument of spec 13 item 4 and Task 6's probe result (panicked or not, where); upstream's bugs (the shadowed `increment_sequences`, the unrecorded CANCEL components, the tree switch) are recorded here only, not reported. Item 5: deferred, outside this plan, to be decided with plan 11's deferred delegate item.
- **Rulings**: every `Ruling:` line of the ledger, verbatim.
- **Mutation runs (plan 12)**: Step 6's table with the observed lines; "No mutation survived" or the exception.
- **Spec notes**: spec 7's big-endian length prefix versus 4.1's "as release 1 pads bundles" (which governs the padding only; release 1's `seal_bytes_as` writes its length little endian); case 20's outcome no longer depends on the drain once revision 7 recorded instances, so its direct assertion ("contacts/pin drained before building") is what pins M71.
- **Adjusted assertions**: every existing assertion a task changed under the generation-bump rule of Global Constraints, with its reason.
- **Deferred findings**: open item 5; the plan 11 note's deferred findings, unchanged; anything a review of this plan deferred.
- **Merge hotspots**: plan 11's list plus Step 5's additions.

- [ ] **Step 9: Commit**

```bash
git add tests/src/za/leak.rs tests/src/za/mod.rs tests/src/webdav/za_variants.rs docs/zero-access/manual-checklist.md CLAUDE.md docs/superpowers/plans/2026-10-17-zero-access-plan12-outcome.md
git commit -m "Plan 12 outcome note, mutation runs, the leak scan of pin records, CLAUDE.md" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```
