# Zero-access plan 8: key material and sealed inbox items Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give key accounts release 2's key material and inbox format: the `pk` wrap, a private key in session keys, attendee token pairs that every rewrite carries forward, sealed inbox items in both states (pending and drained), and inbox read paths that hide pending items and unseal drained ones.

**Architecture:** Release 2 (invitations between key accounts) is split into three plans. Each one produces working, tested software on its own:
- **Plan 8 (this plan): key material and sealed inbox items.** It adds:
  - the `pk` wrap (`pk_wrap`/`pk_unwrap` in `crates/vault/src/keys.rs`);
  - a lazily unwrapped private key in `SessionKeys`, filled by `Server::za_private_key`;
  - token pairs (`TokenPairs`, new `seal/token.rs`) in format 2 of the `X-ZA-EXTRA` bundle, carried forward at both existing event seal sites;
  - sealing and opening of `CalendarEventNotification` trees under the `itip` scope in both states (new `seal/item.rs`);
  - the pending flag bit;
  - the inbox read paths: pending items are skipped and answer 404, drained items are unsealed through a new `CalendarEventNotification` arm of `za_archive_view`.

  Plan 8 sends nothing and drains nothing. `ItipSendStatus::KeyAccount` stays as it is. Only a test helper writes inbox items.
- **Plan 9: the fan-out.** It covers spec section 5, the creation lease of 6.3 in the PUT create branch, and the test-mode hooks of section 11. Plan 9 calls `seal_pending_item`, `TokenPairs`, `event_token_pairs`, `seal_event(.., Some(pairs))` and `EVENT_NOTIFICATION_ZA_PENDING`.
- **Plan 10: the drain, the lease sweep, the end-to-end flows, the key-mode CalDAV variant and the manual checklist.** It covers spec sections 6, 7 and 8 and the flow tests of section 11. Plan 10 calls `Server::za_private_key`, `open_pending_item`, `seal_drained_item`, `archived_item_is_pending` and `tokens_equal` / `TokenPairs::matches`.

The fork diff stays narrow:
- new modules (`seal/token.rs`, `seal/item.rs`) and new functions in fork-owned files (`keys.rs`, `session.rs`, `auth/vault.rs`, `seal/tree.rs`, `seal/event.rs`, `dav/src/common/za.rs`);
- one-call insertions at upstream sites: two seal sites, the PROPFIND loader, the inbox GET.

**Tech Stack:**
- Rust 2024 workspace (Stalwart 0.16.25 fork)
- `x25519-dalek 2.0.1` (`static_secrets`, `zeroize`), `hkdf 0.13`, `chacha20poly1305 0.11`, `subtle 2.6`, `zeroize 1.9`
- rkyv 0.8.18, calcard 0.3, base64 0.23
- the `tests` crate harness (`TestServer`, `DummyWebDavClient`)

**Spec:** `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md`. These sections bind this plan:
- 3: all of key material.
- 5.3 steps 1-3: the item's sealed shape only. Writing items is plan 9.
- 6.6: the inbox as clients see it.
- 9: stored format additions.
- 10: read-path error rules.
- 11: the unit tests, and the integration tests named here.
- 12: invariants 11-15.
- 13: the names settled below.

Section 9's associated-data rows for inbox items were amended alongside this plan (both envelopes and the item bundle bind the UID; item 2 of fix round 1).

`docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (release 1) also binds: sections 6, 7 and 12 for the sealing policy, the carriers and invariants 1-10.

Project rules: `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build it with the `enterprise` feature.
- Per-task compile check in the shipping configuration: `cargo check -p stalwart --no-default-features --features rocks`.
- Unit tests:
  - `cargo test -p vault`
  - `cargo test -p groupware`. The seal tests need no feature (`crates/groupware/Cargo.toml` has only `test_mode` and `enterprise`, and no seal test is behind either).
  - `cargo test -p common auth::vault`. `-p common` without `enterprise` compiles for unit tests; `--features test_mode` does not.
- Integration suite: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`. It runs one server on port 8899. Never run two suites at once, and never in parallel with a mutation build.

Formatting and warnings:
- Run `cargo fmt -p <crate> -- --check` for every touched crate: `vault`, `groupware`, `common`, `dav`, `tests`. This plan does not touch the http crate.
- The build output stays warning-free for new code. The 24 upstream warnings in store, common, jmap and services are known.

Invariants and code rules:
- No edits to generated code (`crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`).
- No new setting, permission or event type. Environment variables only if a knob is needed; none is expected in this plan.
- No stored struct changes layout (invariant 1). Ciphertext rides in existing fields: the `ICalendar` tree, `CalendarEventNotification.flags`, and the sealed `X-ZA-EXTRA` bundle.
- Key material, tokens included, lives in `Zeroizing` buffers with a silent `Debug`. It never appears in logs, traces or errors.
- Non-key accounts take unchanged upstream paths. `None` keys means the stored archive (invariant 9).
- The `pk` wrap is opened only by `open_pending_item`. Plan 10's drain is its only product caller; in plan 8 only tests call it (invariant 11). No read path opens a `pk` wrap.
- New source files start with the SPDX header copied from a neighbour:
  ```
  /*
   * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
   *
   * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
   */
  ```
- Keep the fork diff narrow: new modules plus one-line call insertions at existing sites.

Tests:
- Every za sub-module removes what it creates. In `tests/src/za` no sub-module calls `assert_is_empty`; only the suite end does (`tests/src/za/mod.rs:113-114`, after `destroy_key_accounts`).
  - `CLAUDE.md` says "every sub-module ends with `assert_is_empty`", but `grep -n assert_is_empty tests/src/za/*.rs` hits only `mod.rs:114`.
  - Mid-suite, key1's default calendar and the suite's accounts exist, and `assert_is_empty` also clears the search store and the DAV caches.
  - New modules follow the code: they delete their items and events, and the suite-end `assert_is_empty` proves it.
- Never weaken an assertion to make a test pass.
  - Values read from code and never run are marked "(from code reading)".
  - If a run disagrees with one, assert what the run shows only where the step names an oracle or a fallback, and record a ruling in the task report.
  - Otherwise stop and report.
- Mutation checks are temporary edits under `crates/`, undone at once:
  - Red runs inside Tasks 3, 5 and 6 come before that task's commit, so `git checkout` would also discard the task's work. Undo the edit by restoring the exact code the task's implementation step shows, then confirm with `git diff -- <file>` that only the task's intended change remains.
  - Task 9's runs are on committed code. Revert each one with `git checkout -- <file>`, and confirm `git status --short crates/` is empty before the next edit.

Process:
- Code in this plan was written without a compiler. Small type and import fixes are expected; changes of behaviour are not.
- Work on a feature branch from `main` (for example `plan8-sealed-items`). Do not push.
- Commit messages: subject, blank line, body, then the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. The executor adds the session trailer the harness specifies at run time.

### Names this plan settles (spec 13)

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

## Review Focus

Each line names an input the spec implies but does not walk through, with the behaviour a client would expect. Each has a test in the task that owns the code.

1. **A `calendar-multiget` REPORT on the inbox naming a pending item's href.** The answer is 404 for that href: never its tree, never a 500.
   - Multiget is parented on the `Calendar` collection (`crates/dav/src/request.rs:290-300`).
   - The fork's per-href collection check (`crates/dav/src/common/propfind.rs:1558-1562`) answers 404 before any archive is read.
   - Task 6 pins this, so a later change that lets multiget reach inbox items cannot expose a pending one.
2. **HEAD, and PROPFIND depth 0, on a pending item's href.** Both answer 404, like GET.
   - HEAD shares GET's handler.
   - A PROPFIND whose only item is skipped returns 404 (`propfind.rs:1172-1176`).
   - Task 6.
3. **A `pk`-wrapped item whose pending bit is clear** (a store writer, or a future drain bug). This is release 1 section 10:
   - GET answers 500;
   - the item's PROPFIND entry is a 500 and the rest of the listing is intact;
   - the record is neither deleted nor rewritten.

   Task 3 (unit) and Task 6 (integration).
4. **A legacy plaintext item in a key account's inbox** (no `X-ZA-KEY`, bit clear). It is listed and returned as stored, as release 1 plaintext events are (release 1 section 7.3). Task 3 (unit) and Task 6 (integration).
5. **Rewrites of events that have no pairs:** a release 1 event with a format 1 bundle, and a legacy plaintext event. PUT and PROPPATCH succeed, the display name survives, and `event_token_pairs` still answers `None`. Task 5.

## File map

| File | Change | Task |
|---|---|---|
| `crates/vault/src/keys.rs` | `LABEL_PK`, `AAD_PK_DEK`, `PUBLIC_KEY_LEN`, `KeyError::PublicKey`, `pk_wrap`, `pk_unwrap`, tests | 1 |
| `crates/groupware/Cargo.toml` | `subtle`, `zeroize` | 2 |
| `crates/groupware/src/calendar/seal/token.rs` | new: `TokenPairs`, `tokens_equal` | 2 |
| `crates/groupware/src/calendar/seal/tree.rs` | `FORMAT_V2`, `seal_bytes_as`, `open_bytes_as`, `decode_archive`; `WRAP_PK`, `pk` envelopes | 2, 3 |
| `crates/groupware/src/calendar/seal/event.rs` | `ExtraV2`, `seal_extra`, `open_extra`, `open_root`, `seal_event` pairs parameter, `event_token_pairs` | 2 |
| `crates/groupware/src/calendar/seal/item.rs` | new: item sealing in both states | 3 |
| `crates/groupware/src/calendar/seal/mod.rs` | modules and re-exports | 2, 3 |
| `crates/groupware/src/calendar/mod.rs` | `EVENT_NOTIFICATION_ZA_PENDING` | 3 |
| `crates/vault/src/session.rs` | lazily unwrapped private key | 4 |
| `crates/common/src/auth/vault.rs` | `za_open_private_key`, `Server::za_private_key`, tests | 4 |
| `crates/dav/src/calendar/update.rs` | `None` at both seal sites (2); stored pairs at the update site (5) | 2, 5 |
| `crates/dav/src/calendar/proppatch.rs` | `None` (2); stored pairs (5) | 2, 5 |
| `crates/dav/src/common/za.rs` | `za_item_view`, `za_item_hidden`, the `CalendarEventNotification` arm | 6 |
| `crates/dav/src/common/propfind.rs` | the pending skip in the loader | 6 |
| `crates/dav/src/calendar/scheduling.rs` | GET: 404 for pending, view for drained | 6 |
| `tests/src/utils/za.rs` | `za_unlock`, `plant_sealed_event` (5); `PlantedItem`, `plant_inbox_item`, `rewrite_inbox_item` (6) | 5, 6 |
| `tests/src/za/pairs.rs` | new: pairs survive rewrites | 5 |
| `tests/src/za/inbox.rs` | new: hidden and visible items (6); sync-collection and expiry (7) | 6, 7 |
| `tests/src/za/mod.rs` | wiring before `leak::test` | 5, 6, 7 |
| `tests/src/za/leak.rs` | inbox items in the scan, token check | 8 |
| `docs/superpowers/plans/2026-10-13-zero-access-plan8-outcome.md` | new: mutation table, rulings | 9 |

---

### Task 1: The `pk` wrap in the vault crate

**Files:**
- Modify: `crates/vault/src/keys.rs:23-28` (labels), `:71-86` (`KeyError`), `:148-153` (AAD purposes), after `:233` (new functions), tests module `:241-389`
- Test: `crates/vault/src/keys.rs` (tests module)

**Interfaces:**
- Consumes (existing): `hkdf_expand(ikm, label) -> Secret` (`:112-118`), `wrap_key`, `unwrap_key`, `Secret`, `generate_keypair`, `aad`, `WRAP_LEN`.
- Produces:
  - `pub const AAD_PK_DEK: &str = "pk-dek"`
  - `pub const PUBLIC_KEY_LEN: usize = 32`
  - `KeyError::PublicKey`
  - `pub fn pk_wrap(dek: &Secret, recipient: &[u8; 32], aad: &[u8]) -> Result<([u8; 32], Vec<u8>), KeyError>`: returns the ephemeral public key E and `nonce || ciphertext` (72 bytes).
  - `pub fn pk_unwrap(ephemeral: &[u8; 32], wrapped: &[u8], private_key: &Secret, aad: &[u8]) -> Result<Secret, KeyError>`
  - Task 3 wraps these into the `X-ZA-KEY` envelope. Plan 9 seals through Task 3's `seal_pending_item`; plan 10 opens through `open_pending_item`.

Background:
- `hkdf_expand` passes its `label` argument to HKDF as the info input (`hk.expand(label, ..)`, `:115`).
- `derive_app_kek` (`:138-142`) already builds a longer info by appending to its label.
- So the `pk` derivation appends `E || P` to `LABEL_PK` the same way and calls `hkdf_expand` unchanged. No info-taking sibling is added: `hkdf_expand` already takes the whole info.
- Steps of the wrap (spec 3.1):
  1. Generate an ephemeral keypair (e, E).
  2. Compute `s = X25519(e, P)`.
  3. Derive the wrapping key `HKDF(s, LABEL_PK || E || P)`.
  4. Seal the DEK with `wrap_key` under the associated data the caller passes. Task 3 passes `aad(AAD_PK_DEK, r) || "|" || uid`.
  5. Zero e: `StaticSecret` zeroes on drop (`zeroize` feature).
- A non-contributory shared secret is refused on both sides (`SharedSecret::was_contributory`). A low-order public key, such as all zeros in a corrupted record, would make s known to anyone, so it becomes `KeyError::PublicKey`, not a silently readable wrap.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `crates/vault/src/keys.rs`:

```rust
    #[test]
    fn pk_wrap_round_trips_for_the_recipient_only() {
        let (public, private) = generate_keypair();
        let dek = Secret::random();
        let aad7 = aad(AAD_PK_DEK, 7);
        let (ephemeral, wrapped) = pk_wrap(&dek, &public, &aad7).unwrap();
        assert_eq!(wrapped.len(), WRAP_LEN);
        assert_eq!(
            pk_unwrap(&ephemeral, &wrapped, &private, &aad7)
                .unwrap()
                .as_bytes(),
            dek.as_bytes()
        );
        // Another recipient's private key.
        let (_, other) = generate_keypair();
        assert_eq!(
            pk_unwrap(&ephemeral, &wrapped, &other, &aad7).err(),
            Some(KeyError::Aead)
        );
        // Associated data naming another account or another purpose.
        for wrong in [aad(AAD_PK_DEK, 8), aad(AAD_PRIVATE_KEY, 7)] {
            assert_eq!(
                pk_unwrap(&ephemeral, &wrapped, &private, &wrong).err(),
                Some(KeyError::Aead)
            );
        }
        // The wrap moved under another ephemeral key.
        let (other_ephemeral, _) = pk_wrap(&dek, &public, &aad7).unwrap();
        assert_eq!(
            pk_unwrap(&other_ephemeral, &wrapped, &private, &aad7).err(),
            Some(KeyError::Aead)
        );
        // Tampered and truncated ciphertext.
        let mut tampered = wrapped.clone();
        tampered[40] ^= 1;
        assert!(pk_unwrap(&ephemeral, &tampered, &private, &aad7).is_err());
        assert!(pk_unwrap(&ephemeral, &wrapped[..WRAP_LEN - 1], &private, &aad7).is_err());
    }

    #[test]
    fn two_pk_wraps_of_one_dek_differ() {
        let (public, private) = generate_keypair();
        let dek = Secret::random();
        let aad1 = aad(AAD_PK_DEK, 1);
        let (e1, w1) = pk_wrap(&dek, &public, &aad1).unwrap();
        let (e2, w2) = pk_wrap(&dek, &public, &aad1).unwrap();
        assert_ne!(e1, e2, "a fresh ephemeral key per wrap");
        assert_ne!(w1, w2);
        for (e, w) in [(e1, w1), (e2, w2)] {
            assert_eq!(
                pk_unwrap(&e, &w, &private, &aad1).unwrap().as_bytes(),
                dek.as_bytes()
            );
        }
    }

    #[test]
    fn pk_wrap_refuses_low_order_public_keys() {
        let (public, private) = generate_keypair();
        let dek = Secret::random();
        let aad1 = aad(AAD_PK_DEK, 1);
        assert_eq!(
            pk_wrap(&dek, &[0u8; PUBLIC_KEY_LEN], &aad1).err(),
            Some(KeyError::PublicKey)
        );
        let (_, wrapped) = pk_wrap(&dek, &public, &aad1).unwrap();
        assert_eq!(
            pk_unwrap(&[0u8; PUBLIC_KEY_LEN], &wrapped, &private, &aad1).err(),
            Some(KeyError::PublicKey)
        );
    }

    #[test]
    fn pk_info_is_the_label_then_both_public_keys() {
        let shared = [5u8; 32];
        let (e, p) = ([1u8; 32], [2u8; 32]);
        let mut info = LABEL_PK.to_vec();
        info.extend_from_slice(&e);
        info.extend_from_slice(&p);
        let key = pk_wrapping_key(&shared, &e, &p);
        assert_eq!(key.as_bytes(), hkdf_expand(&shared, &info).as_bytes());
        assert_ne!(key.as_bytes(), pk_wrapping_key(&shared, &p, &e).as_bytes());
        assert_ne!(key.as_bytes(), hkdf_expand(&shared, LABEL_PK).as_bytes());
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p vault keys::tests::pk 2>&1 | tail -20`
Expected: compile error, `cannot find function 'pk_wrap' in this scope` (and `AAD_PK_DEK`, `LABEL_PK`, `pk_wrapping_key`, `PUBLIC_KEY_LEN`).

- [ ] **Step 3: Implement**

In the label block (`:23-28`), add after `LABEL_EVENTS`:

```rust
const LABEL_PK: &[u8] = b"za/v1/pk";
```

Add a variant to `KeyError` (`:71-76`) and its `Display` arm (`:78-86`):

```rust
pub enum KeyError {
    Argon2,
    Length,
    Aead,
    /// A public key whose shared secret is not contributory (low order).
    PublicKey,
}
```

```rust
            KeyError::PublicKey => f.write_str("invalid public key"),
```

After `AAD_PRIVATE_KEY` (`:153`):

```rust
/// AAD purpose of a DEK wrapped to a recipient's public key (invitations
/// spec 3.1). `aad` adds the `za/v1|` prefix.
pub const AAD_PK_DEK: &str = "pk-dek";
/// Length of an X25519 public key.
pub const PUBLIC_KEY_LEN: usize = 32;
```

After `generate_keypair` (`:226-233`):

```rust
/// Wrapping key of a `pk` wrap: HKDF over the shared secret with the info
/// `za/v1/pk || E || P` (ephemeral, then recipient public key), built by
/// appending to the label as `derive_app_kek` does.
fn pk_wrapping_key(
    shared: &[u8; 32],
    ephemeral: &[u8; PUBLIC_KEY_LEN],
    recipient: &[u8; PUBLIC_KEY_LEN],
) -> Secret {
    let mut info = Vec::with_capacity(LABEL_PK.len() + 2 * PUBLIC_KEY_LEN);
    info.extend_from_slice(LABEL_PK);
    info.extend_from_slice(ephemeral);
    info.extend_from_slice(recipient);
    hkdf_expand(shared, &info)
}

/// Invitations spec 3.1: wraps `dek` to the X25519 public key `recipient`.
/// Returns the ephemeral public key and `nonce || ciphertext`
/// (`WRAP_LEN` bytes). The ephemeral private key is zeroed on return.
pub fn pk_wrap(
    dek: &Secret,
    recipient: &[u8; PUBLIC_KEY_LEN],
    aad: &[u8],
) -> Result<([u8; PUBLIC_KEY_LEN], Vec<u8>), KeyError> {
    let ephemeral = x25519_dalek::StaticSecret::from(*Secret::random().as_bytes());
    let ephemeral_public = x25519_dalek::PublicKey::from(&ephemeral).to_bytes();
    let shared = ephemeral.diffie_hellman(&x25519_dalek::PublicKey::from(*recipient));
    if !shared.was_contributory() {
        return Err(KeyError::PublicKey);
    }
    let wrapping = pk_wrapping_key(shared.as_bytes(), &ephemeral_public, recipient);
    Ok((ephemeral_public, wrap_key(dek, &wrapping, aad)))
}

/// Opens a `pk_wrap` with the recipient's private key. Only the drain calls
/// this (invariant 11).
pub fn pk_unwrap(
    ephemeral: &[u8; PUBLIC_KEY_LEN],
    wrapped: &[u8],
    private_key: &Secret,
    aad: &[u8],
) -> Result<Secret, KeyError> {
    let secret = x25519_dalek::StaticSecret::from(*private_key.as_bytes());
    let recipient = x25519_dalek::PublicKey::from(&secret).to_bytes();
    let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(*ephemeral));
    if !shared.was_contributory() {
        return Err(KeyError::PublicKey);
    }
    let wrapping = pk_wrapping_key(shared.as_bytes(), ephemeral, &recipient);
    unwrap_key(wrapped, &wrapping, aad)
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p vault 2>&1 | tail -5`
Expected: PASS, 29 tests (25 before plus 4).

- [ ] **Step 5: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p vault -- --check
git add crates/vault/src/keys.rs
git commit -m "Vault: the pk wrap to an account's X25519 public key"   # body and trailer per Global Constraints
```

---

### Task 2: Token pairs and format 2 of the `X-ZA-EXTRA` bundle

**Files:**
- Modify: `crates/groupware/Cargo.toml` (dependencies)
- Create: `crates/groupware/src/calendar/seal/token.rs`
- Modify: `crates/groupware/src/calendar/seal/tree.rs:14-28` (imports, constants), `:169-216` (`seal_bytes`, `open_bytes`, `open_archive`)
- Modify: `crates/groupware/src/calendar/seal/event.rs:16-38` (imports, `Extra`), `:96-182` (`seal_event`, `unseal_event`), tests `:215-547`
- Modify: `crates/groupware/src/calendar/seal/mod.rs:9-18`
- Modify: `crates/dav/src/calendar/update.rs:334`, `:494`, `crates/dav/src/calendar/proppatch.rs:254` (append `, None`)

**Interfaces:**
- Consumes: `seal_tree`, `open_key_envelope`, `has_stray_carriers`, `is_carrier`, `entry_text`, `text_entry` (`tree.rs`).
- Produces (Tasks 3, 5 and plans 9, 10 use these):
  - `groupware::calendar::seal::{TokenPairs, TOKEN_LEN, tokens_equal}`:
    - `TokenPairs::new()`
    - `insert(&mut self, address: &str, token: [u8; 16])` (adds or replaces; addresses compare ASCII case-insensitively)
    - `get(&self, address) -> Option<&[u8; 16]>`
    - `matches(&self, address, &[u8; 16]) -> bool` (constant time)
    - `only(&self, address) -> Option<TokenPairs>`
    - `len()`, `is_empty()`
    - crate-internal `from_vec`, `to_vec`
    - `tokens_equal(&[u8; 16], &[u8; 16]) -> bool`
  - `seal_event(event: &mut CalendarEvent, keys: &SessionKeys, account_id: u32, pairs: Option<&TokenPairs>) -> Result<(), SealError>`
  - `event_token_pairs(stored: &Archive<AlignedBytes>, keys: &SessionKeys, account_id: u32) -> Result<Option<TokenPairs>, SealError>`
  - crate-internal for Task 3: `tree::{FORMAT_V2, seal_bytes_as, open_bytes_as, decode_archive}`, `event::{seal_extra, open_extra, OpenedExtra}`

Background:
- `Extra` (`event.rs:33-38`) is decoded with a checked rkyv read, so a bundle that carries pairs needs a new plaintext struct and a new format byte (spec 9).
- Format 1 bundles keep decoding as `Extra`, and `seal_event` still writes format 1, byte for byte as release 1 does, when there are no pairs.
- Format 2 binds its format byte into the associated data (`<aad>|\x02`). A flipped byte then fails authentication instead of decoding one struct as the other. Format 1 is unchanged.
- Removal bundles (`X-ZA-SEALED`) and collection bundles accept format 1 only.
- Buffers that hold tokens are zeroed: the serialized plaintext, the padded buffer, the decrypted copy and the aligned decode copy.
- Changing `seal_event`'s signature breaks its three dav callers. In this task they pass `None`. Task 5 passes the stored pairs at the two rewrite sites.

- [ ] **Step 1: Dependencies, and the token tests**

In `crates/groupware/Cargo.toml`, under `[dependencies]`, after `base64 = "0.23"`:

```toml
subtle = "2.6"
zeroize = "1.9"
```

Create `crates/groupware/src/calendar/seal/token.rs` with the SPDX header and only this tests module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn pairs() -> TokenPairs {
        let mut pairs = TokenPairs::new();
        pairs.insert("Bob@Example.com", [1; TOKEN_LEN]);
        pairs.insert("carol@example.com", [2; TOKEN_LEN]);
        pairs
    }

    #[test]
    fn pairs_are_found_by_address_and_replaced() {
        assert!(TokenPairs::new().is_empty());
        let mut pairs = pairs();
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs.get("bob@example.com"),
            Some(&[1; TOKEN_LEN]),
            "addresses compare ASCII case-insensitively"
        );
        pairs.insert("bob@example.com", [3; TOKEN_LEN]);
        assert_eq!(pairs.len(), 2, "a second insert replaces");
        assert_eq!(pairs.get("BOB@example.com"), Some(&[3; TOKEN_LEN]));
        assert!(pairs.get("dave@example.com").is_none());
    }

    #[test]
    fn matches_compares_the_token_of_that_address() {
        let pairs = pairs();
        assert!(pairs.matches("bob@example.com", &[1; TOKEN_LEN]));
        assert!(!pairs.matches("bob@example.com", &[2; TOKEN_LEN]));
        assert!(!pairs.matches("carol@example.com", &[1; TOKEN_LEN]));
        assert!(!pairs.matches("dave@example.com", &[1; TOKEN_LEN]));
        for i in 0..TOKEN_LEN {
            let mut token = [1; TOKEN_LEN];
            token[i] ^= 0x80;
            assert!(!pairs.matches("bob@example.com", &token), "byte {i}");
        }
        assert!(tokens_equal(&[7; TOKEN_LEN], &[7; TOKEN_LEN]));
        assert!(!tokens_equal(&[7; TOKEN_LEN], &[8; TOKEN_LEN]));
    }

    #[test]
    fn only_keeps_one_attendees_pair() {
        let only = pairs().only("CAROL@example.com").unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(only.get("carol@example.com"), Some(&[2; TOKEN_LEN]));
        assert!(only.get("bob@example.com").is_none());
        assert!(pairs().only("dave@example.com").is_none());
    }

    #[test]
    fn debug_prints_no_address_or_token() {
        let mut pairs = TokenPairs::new();
        pairs.insert("secret-address@example.com", [0xAB; TOKEN_LEN]);
        let text = format!("{pairs:?}");
        assert_eq!(text, "TokenPairs(1)");
    }
}
```

In `crates/groupware/src/calendar/seal/mod.rs`, add `pub mod token;` after `pub mod policy;`.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p groupware seal::token 2>&1 | tail -10`
Expected: compile error, `cannot find type 'TokenPairs'`.

- [ ] **Step 3: Implement `TokenPairs`**

Prepend to `token.rs`, after the SPDX header:

```rust
//! Attendee token pairs (invitations spec 3.3). One random 16-byte token per
//! attendee per event authenticates that attendee's replies. Pairs travel
//! only inside sealed `X-ZA-EXTRA` bundles: all of the organizer's pairs on
//! the organizer's event, the attendee's own pair on the attendee's copy and
//! on a pending REQUEST or CANCEL item. Never logged.

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Length of one attendee token.
pub const TOKEN_LEN: usize = 16;

/// (attendee address, token) pairs, zeroed on drop, never printed. Addresses
/// compare ASCII case-insensitively; callers store them as they match
/// attendees.
#[derive(Clone, Default)]
pub struct TokenPairs(Zeroizing<Vec<(String, [u8; TOKEN_LEN])>>);

impl TokenPairs {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_vec(pairs: Vec<(String, [u8; TOKEN_LEN])>) -> Self {
        TokenPairs(Zeroizing::new(pairs))
    }

    /// A plain copy for serialization; the caller zeroes it after use.
    pub(crate) fn to_vec(&self) -> Vec<(String, [u8; TOKEN_LEN])> {
        self.0.to_vec()
    }

    fn position(&self, address: &str) -> Option<usize> {
        self.0.iter().position(|(a, _)| a.eq_ignore_ascii_case(address))
    }

    /// Adds the pair of `address`, replacing a stored one (an accepted
    /// REQUEST replaces the attendee's pair, spec 6.4).
    pub fn insert(&mut self, address: &str, token: [u8; TOKEN_LEN]) {
        match self.position(address) {
            Some(index) => self.0[index].1 = token,
            None => self.0.push((address.to_string(), token)),
        }
    }

    pub fn get(&self, address: &str) -> Option<&[u8; TOKEN_LEN]> {
        self.position(address).map(|index| &self.0[index].1)
    }

    /// Spec 3.3, "Verification": whether `token` is the token of `address`,
    /// compared in constant time. False when `address` has no pair.
    pub fn matches(&self, address: &str, token: &[u8; TOKEN_LEN]) -> bool {
        self.get(address)
            .is_some_and(|stored| tokens_equal(stored, token))
    }

    /// The pair of `address` alone: what an item sent to that attendee
    /// carries (spec 3.3, "Attendee side").
    pub fn only(&self, address: &str) -> Option<TokenPairs> {
        self.position(address)
            .map(|index| TokenPairs::from_vec(vec![self.0[index].clone()]))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for TokenPairs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TokenPairs({})", self.0.len())
    }
}

/// Constant-time equality of two tokens.
pub fn tokens_equal(a: &[u8; TOKEN_LEN], b: &[u8; TOKEN_LEN]) -> bool {
    a.ct_eq(b).into()
}
```

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p groupware seal::token 2>&1 | tail -5`
Expected: PASS, 4 tests.

- [ ] **Step 5: Write the failing bundle tests and update the existing calls**

Every existing `seal_event` call in `event.rs`'s tests module gains `None` as its fourth argument. There are 13 such calls, at `:270`, `:318`, `:319`, `:327`, `:336`, `:359`, `:378`, `:416`, `:448`, `:452`, `:476`, `:489` and `:518`. Run this before appending the new tests below, or the count includes a new call:

```bash
perl -pi -e 's/\bseal_event\((&mut \w+, .*?), 9\)/seal_event($1, 9, None)/' crates/groupware/src/calendar/seal/event.rs
grep -c 'seal_event(&mut [a-z]*, .*, 9, None)' crates/groupware/src/calendar/seal/event.rs   # expect 13
```

Append inside `event.rs`'s `mod tests`. `TokenPairs`, `TOKEN_LEN`, `FORMAT_V1`, `FORMAT_V2` and `DeadProperty` come through `use super::*` once Step 8's imports are in.

```rust
    fn two_pairs() -> TokenPairs {
        let mut pairs = TokenPairs::new();
        pairs.insert("a@example.com", [1; TOKEN_LEN]);
        pairs.insert("b@example.com", [2; TOKEN_LEN]);
        pairs
    }

    /// The format byte of the root's `X-ZA-EXTRA` bundle.
    fn extra_format(event: &CalendarEvent) -> u8 {
        let root = &event.data.event.components[0];
        let extra = &root.entries[root.entries.len() - 2];
        assert!(is_carrier(extra, EXTRA_PROP));
        STANDARD.decode(entry_text(extra).unwrap()).unwrap()[0]
    }

    fn bare_event() -> CalendarEvent {
        let mut bare = event();
        bare.display_name = None;
        bare.dead_properties = DeadProperty::default();
        bare
    }

    #[test]
    fn pairs_round_trip_through_a_format_2_bundle() {
        let keys = keys();
        let original = event();
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9, Some(&two_pairs())).unwrap();
        assert_eq!(extra_format(&sealed), FORMAT_V2);
        let pairs = event_token_pairs(&archive(&sealed), &keys, 9)
            .unwrap()
            .expect("pairs");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs.get("a@example.com"), Some(&[1; TOKEN_LEN]));
        assert_eq!(pairs.get("b@example.com"), Some(&[2; TOKEN_LEN]));
        // The view restores the extra fields and the tree; pairs are not in it.
        let mut back = sealed.clone();
        unseal_event(&mut back, &keys, 9).unwrap();
        assert_eq!(back, original);
        // Pairs alone, with no display name or dead property, get a bundle.
        let mut bare = bare_event();
        seal_event(&mut bare, &keys, 9, Some(&two_pairs())).unwrap();
        assert_eq!(extra_format(&bare), FORMAT_V2);
        assert_eq!(
            event_token_pairs(&archive(&bare), &keys, 9)
                .unwrap()
                .map(|p| p.len()),
            Some(2)
        );
    }

    #[test]
    fn format_1_bundles_read_as_before_with_no_pairs() {
        let keys = keys();
        for pairs in [None, Some(TokenPairs::new())] {
            let original = event();
            let mut sealed = original.clone();
            seal_event(&mut sealed, &keys, 9, pairs.as_ref()).unwrap();
            assert_eq!(extra_format(&sealed), FORMAT_V1);
            assert!(
                event_token_pairs(&archive(&sealed), &keys, 9)
                    .unwrap()
                    .is_none()
            );
            let mut back = sealed.clone();
            unseal_event(&mut back, &keys, 9).unwrap();
            assert_eq!(back, original);
        }
        // Neither extra fields nor pairs: no bundle at all, as in release 1.
        let mut bare = bare_event();
        seal_event(&mut bare, &keys, 9, None).unwrap();
        assert!(
            !bare.data.event.components[0]
                .entries
                .iter()
                .any(|e| is_carrier(e, EXTRA_PROP))
        );
        assert!(
            event_token_pairs(&archive(&bare), &keys, 9)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_flipped_format_byte_fails_authentication() {
        let keys = keys();
        for (pairs, from, to) in [
            (Some(two_pairs()), FORMAT_V2, FORMAT_V1),
            (None, FORMAT_V1, FORMAT_V2),
        ] {
            let mut sealed = event();
            seal_event(&mut sealed, &keys, 9, pairs.as_ref()).unwrap();
            let n = root_entries(&mut sealed).len();
            let extra = &mut root_entries(&mut sealed)[n - 2];
            let mut raw = STANDARD.decode(entry_text(extra).unwrap()).unwrap();
            assert_eq!(raw[0], from);
            raw[0] = to;
            extra.values = vec![ICalendarValue::Text(STANDARD.encode(raw))];
            assert_eq!(
                event_token_pairs(&archive(&sealed), &keys, 9).err(),
                Some(SealError::Aead)
            );
            assert_eq!(unseal_event(&mut sealed, &keys, 9), Err(SealError::Aead));
        }
    }

    #[test]
    fn token_pairs_of_plaintext_moved_and_foreign_bundles() {
        let keys = keys();
        assert!(
            event_token_pairs(&archive(&event()), &keys, 9)
                .unwrap()
                .is_none(),
            "a plaintext event has no pairs"
        );
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9, Some(&two_pairs())).unwrap();
        let mut swapped = sealed.clone();
        let n = root_entries(&mut swapped).len();
        root_entries(&mut swapped).swap(n - 3, n - 2);
        assert_eq!(
            event_token_pairs(&archive(&swapped), &keys, 9).err(),
            Some(SealError::Structure("misplaced carrier"))
        );
        assert_eq!(
            event_token_pairs(
                &archive(&sealed),
                &SessionKeys::new(9, 1, Secret::random()),
                9
            )
            .err(),
            Some(SealError::Aead)
        );
    }
```

- [ ] **Step 6: Run to see them fail**

Run: `cargo test -p groupware seal::event 2>&1 | tail -10`
Expected: compile errors. `seal_event` takes 3 arguments but 4 were supplied, and `event_token_pairs`, `FORMAT_V2` and `TokenPairs` are not in scope.

- [ ] **Step 7: Format 2 in `tree.rs`**

Imports (`:14-20`): add `use std::borrow::Cow;` and `use zeroize::{Zeroize, Zeroizing};`.

After `pub(crate) const FORMAT_V1: u8 = 1;` (`:25`):

```rust
/// Format of an `X-ZA-EXTRA` bundle that carries token pairs (invitations
/// spec 9). Its associated data ends with `|` and this byte, so a flipped
/// format byte fails authentication instead of decoding one plaintext
/// struct as the other.
pub(crate) const FORMAT_V2: u8 = 2;
```

Replace `seal_bytes`, `open_bytes` and `open_archive` (`:169-216`) with:

```rust
/// The associated data a bundle of `format` is sealed under.
fn format_aad(format: u8, aad: &[u8]) -> Cow<'_, [u8]> {
    if format == FORMAT_V1 {
        Cow::Borrowed(aad)
    } else {
        let mut out = Vec::with_capacity(aad.len() + 2);
        out.extend_from_slice(aad);
        out.push(b'|');
        out.push(format);
        Cow::Owned(out)
    }
}

/// Length prefix, zero padding to 256 bytes, XChaCha20-Poly1305, base64 of
/// `format || nonce || ciphertext`.
pub fn seal_bytes(dek: &Secret, aad: &[u8], plaintext: &[u8]) -> String {
    seal_bytes_as(FORMAT_V1, dek, aad, plaintext)
}

/// `seal_bytes` with an explicit format byte (`FORMAT_V1` or `FORMAT_V2`).
pub(super) fn seal_bytes_as(format: u8, dek: &Secret, aad: &[u8], plaintext: &[u8]) -> String {
    // Capacity covers the padding, so `resize` never reallocates a copy.
    let mut padded = Zeroizing::new(Vec::with_capacity(4 + plaintext.len() + PAD));
    padded.extend_from_slice(&(plaintext.len() as u32).to_le_bytes());
    padded.extend_from_slice(plaintext);
    let target = padded.len().div_ceil(PAD) * PAD;
    padded.resize(target, 0);
    let mut out = Vec::with_capacity(1 + padded.len() + 40);
    out.push(format);
    out.extend_from_slice(&seal(dek, &format_aad(format, aad), &padded));
    STANDARD.encode(out)
}

/// Opens a format 1 bundle.
pub fn open_bytes(dek: &Secret, aad: &[u8], text: &str) -> Result<Zeroizing<Vec<u8>>, SealError> {
    open_bytes_as(&[FORMAT_V1], dek, aad, text).map(|(_, plain)| plain)
}

/// Opens a bundle whose format byte is one of `formats`; returns that byte
/// and the plaintext.
pub(super) fn open_bytes_as(
    formats: &[u8],
    dek: &Secret,
    aad: &[u8],
    text: &str,
) -> Result<(u8, Zeroizing<Vec<u8>>), SealError> {
    let raw = STANDARD
        .decode(text.trim())
        .map_err(|_| SealError::Decode)?;
    let Some((&format, sealed)) = raw
        .split_first()
        .filter(|(format, _)| formats.contains(*format))
    else {
        return Err(SealError::Format);
    };
    let padded = open(dek, &format_aad(format, aad), sealed).map_err(|_| SealError::Aead)?;
    let len = padded
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(|b| u32::from_le_bytes(b) as usize)
        .ok_or(SealError::Format)?;
    padded
        .get(4..)
        .and_then(|rest| rest.get(..len))
        .map(|s| (format, Zeroizing::new(s.to_vec())))
        .ok_or(SealError::Format)
}

/// A checked rkyv decode of opened plaintext. rkyv validates alignment and
/// decrypted bytes carry no guarantee, so they are copied into an aligned
/// buffer first; the copy is zeroed after decoding.
pub(super) fn decode_archive<T>(plain: &[u8]) -> Result<T, SealError>
where
    T: rkyv::Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rkyv::rancor::Error>>
        + rkyv::Deserialize<T, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    let mut aligned = rkyv::util::AlignedVec::<16>::with_capacity(plain.len());
    aligned.extend_from_slice(plain);
    let decoded = rkyv::from_bytes::<T, rkyv::rancor::Error>(&aligned).map_err(|_| SealError::Format);
    aligned.as_mut_slice().zeroize();
    decoded
}

/// `open_bytes`, then `decode_archive`.
pub(super) fn open_archive<T>(dek: &Secret, aad: &[u8], text: &str) -> Result<T, SealError>
where
    T: rkyv::Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rkyv::rancor::Error>>
        + rkyv::Deserialize<T, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    decode_archive(&open_bytes(dek, aad, text)?)
}
```

- [ ] **Step 8: Format 2 in `event.rs`, the new parameter, `event_token_pairs`**

Replace the `use super::tree::{..}` import (`:16-19`) with:

```rust
use super::{
    token::{TOKEN_LEN, TokenPairs},
    tree::{
        EXTRA_PROP, FORMAT_V1, FORMAT_V2, KEY_PROP, SealError, decode_archive, entry_text,
        has_stray_carriers, is_carrier, open_bytes_as, open_key_envelope, seal_bytes,
        seal_bytes_as, seal_key_envelope, seal_tree, text_entry, unseal_tree,
    },
};
use zeroize::Zeroize;
```

After `struct Extra` (`:33-38`), whose doc gains "(format 1, release 1)":

```rust
/// Format 2 (invitations spec 9): `Extra`'s fields and the token pairs. No
/// `Debug`: it holds tokens.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct ExtraV2 {
    display_name: Option<String>,
    dead_properties: DeadProperty,
    pairs: Vec<(String, [u8; TOKEN_LEN])>,
}

/// An opened `X-ZA-EXTRA` bundle of either format.
pub(super) struct OpenedExtra {
    pub(super) display_name: Option<String>,
    pub(super) dead_properties: DeadProperty,
    /// `None` for a format 1 bundle.
    pub(super) pairs: Option<TokenPairs>,
}

/// Seals the extra fields and the pairs into one bundle, or `None` when
/// there is nothing to carry (no bundle, as in release 1). Format 2 only
/// when `pairs` holds a pair; otherwise format 1, exactly as release 1
/// writes it.
pub(super) fn seal_extra(
    dek: &Secret,
    aad: &[u8],
    display_name: Option<String>,
    dead_properties: DeadProperty,
    pairs: Option<&TokenPairs>,
) -> Option<String> {
    match pairs.filter(|pairs| !pairs.is_empty()) {
        Some(pairs) => {
            let mut extra = ExtraV2 {
                display_name,
                dead_properties,
                pairs: pairs.to_vec(),
            };
            let mut plain = rkyv::to_bytes::<rkyv::rancor::Error>(&extra)
                .expect("rkyv serialization of in-memory extra cannot fail");
            let text = seal_bytes_as(FORMAT_V2, dek, aad, &plain);
            plain.as_mut_slice().zeroize();
            extra.pairs.zeroize();
            Some(text)
        }
        None if display_name.is_some() || !dead_properties.0.is_empty() => {
            let extra = Extra {
                display_name,
                dead_properties,
            };
            let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&extra)
                .expect("rkyv serialization of in-memory extra cannot fail");
            Some(seal_bytes(dek, aad, &plain))
        }
        None => None,
    }
}

/// Opens an `X-ZA-EXTRA` bundle of either format.
pub(super) fn open_extra(dek: &Secret, aad: &[u8], text: &str) -> Result<OpenedExtra, SealError> {
    match open_bytes_as(&[FORMAT_V1, FORMAT_V2], dek, aad, text)? {
        (FORMAT_V1, plain) => {
            let extra = decode_archive::<Extra>(&plain)?;
            Ok(OpenedExtra {
                display_name: extra.display_name,
                dead_properties: extra.dead_properties,
                pairs: None,
            })
        }
        (_, plain) => {
            let extra = decode_archive::<ExtraV2>(&plain)?;
            Ok(OpenedExtra {
                display_name: extra.display_name,
                dead_properties: extra.dead_properties,
                pairs: Some(TokenPairs::from_vec(extra.pairs)),
            })
        }
    }
}
```

Replace `seal_event` (`:96-141`). The doc comment gains a paragraph; the checks and the envelope are unchanged:

```rust
/// Seals an event immediately before the store write (spec 8.1). Time
/// ranges and alarms were computed on the plaintext tree by the caller.
///
/// `pairs` are the token pairs the bundle carries (invitations spec 3.3):
/// on every rewrite, the stored event's pairs (`event_token_pairs`), because
/// the unsealed view the caller edits cannot hold them; `None` for a new
/// event until the fan-out (plan 9) hands out tokens.
///
/// There is no "already sealed" check: a body whose root ends with
/// `X-ZA-KEY` is an ordinary `X-` property here and is sealed and restored
/// unchanged. Each write path seals exactly once.
pub fn seal_event(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
    pairs: Option<&TokenPairs>,
) -> Result<(), SealError> {
    let root = event
        .data
        .event
        .components
        .first()
        .ok_or(SealError::Structure("empty tree"))?;
    if root.component_type != ICalendarComponentType::VCalendar {
        return Err(SealError::Structure("root is not VCALENDAR"));
    }
    if keys.account_id != account_id {
        return Err(SealError::Structure("keys of another account"));
    }
    let uid = uid_of(event);
    let dek = Secret::random();
    seal_tree(&mut event.data.event, &dek, account_id, &uid);

    if let Some(text) = seal_extra(
        &dek,
        &extra_aad(account_id, &uid),
        event.display_name.take(),
        std::mem::take(&mut event.dead_properties),
        pairs,
    ) {
        event.data.event.components[0]
            .entries
            .push(text_entry(EXTRA_PROP, text));
    }

    let envelope = seal_key_envelope(&dek, keys.ewk(), &key_aad(account_id, &uid));
    event.data.event.components[0]
        .entries
        .push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}
```

Replace `unseal_event` (`:143-182`) with a shared root opener, `unseal_event` and `event_token_pairs`:

```rust
/// Pops and opens a sealed event's key envelope and `X-ZA-EXTRA` bundle,
/// after checking that no carrier is misplaced. Returns the DEK, the UID
/// and the opened bundle. The caller checked `is_sealed`.
fn open_root(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<(Secret, String, Option<OpenedExtra>), SealError> {
    let uid = uid_of(event);
    let root = &mut event.data.event.components[0];
    let key_entry = root
        .entries
        .pop()
        .ok_or(SealError::Structure("key envelope"))?;
    let envelope = STANDARD
        .decode(entry_text(&key_entry).ok_or(SealError::Format)?)
        .map_err(|_| SealError::Decode)?;
    let dek = open_key_envelope(&envelope, keys.ewk(), &key_aad(account_id, &uid))?;
    let extra = root.entries.pop_if(|e| is_carrier(e, EXTRA_PROP));
    if has_stray_carriers(&event.data.event) {
        return Err(SealError::Structure("misplaced carrier"));
    }
    let extra = match extra {
        Some(carrier) => Some(open_extra(
            &dek,
            &extra_aad(account_id, &uid),
            entry_text(&carrier).ok_or(SealError::Format)?,
        )?),
        None => None,
    };
    Ok((dek, uid, extra))
}

/// Unseals an event in place.
///
/// An event whose root has no trailing `X-ZA-KEY` is plaintext (written
/// before the account held keys) and is left unchanged: reads tolerate it,
/// and the next write seals it. Token pairs are not part of the view.
///
/// On `Err` the event is partly modified and must be discarded.
pub fn unseal_event(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<(), SealError> {
    if !is_sealed(event) {
        return Ok(());
    }
    let (dek, uid, extra) = open_root(event, keys, account_id)?;
    if let Some(extra) = extra {
        event.display_name = extra.display_name;
        event.dead_properties = extra.dead_properties;
    }
    unseal_tree(&mut event.data.event, &dek, account_id, &uid)
}

/// Invitations spec 3.3: the token pairs of a stored event, opening only its
/// key envelope and `X-ZA-EXTRA` bundle. `None` for a plaintext event, an
/// event without a bundle, and a format 1 bundle.
pub fn event_token_pairs(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Option<TokenPairs>, SealError> {
    let mut event = stored
        .deserialize::<CalendarEvent>()
        .map_err(|_| SealError::Format)?;
    if !is_sealed(&event) {
        return Ok(None);
    }
    let (_, _, extra) = open_root(&mut event, keys, account_id)?;
    Ok(extra.and_then(|extra| extra.pairs))
}
```

In `seal/mod.rs`, replace the `event` re-export and add the token one:

```rust
pub use event::{
    archived_event_is_sealed, event_token_pairs, seal_event, unseal_event, unseal_event_archive,
};
pub use token::{TOKEN_LEN, TokenPairs, tokens_equal};
```

- [ ] **Step 9: Keep the dav callers compiling**

Append `, None` as the fourth argument at the three call sites. Task 5 changes the first and the third:
- `crates/dav/src/calendar/update.rs:334`: `seal_event(&mut new_event, keys, account_id, None)`
- `crates/dav/src/calendar/update.rs:494`: `seal_event(&mut event, keys, account_id, None)`
- `crates/dav/src/calendar/proppatch.rs:254`: `seal_event(&mut new_event, keys, account_id, None)`

- [ ] **Step 10: Run everything this touches**

Run: `cargo test -p groupware 2>&1 | tail -5`
Expected: PASS. 51 tests before this task plus 8 new (4 token, 4 event), 59. The existing envelope, tamper and garbage tests keep their expected errors.

Run: `cargo check -p stalwart --no-default-features --features rocks 2>&1 | grep -E "^(warning|error)" -A3 | grep -E "crates/(groupware|dav)/" || echo clean`
Expected: `clean`.

- [ ] **Step 11: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p groupware -- --check
cargo fmt -p dav -- --check
git add crates/groupware/Cargo.toml crates/groupware/src/calendar/seal/ crates/dav/src/calendar/update.rs crates/dav/src/calendar/proppatch.rs
git commit -m "Seal: token pairs in a format 2 X-ZA-EXTRA bundle"
```

---

### Task 3: Sealed inbox items in both states

**Files:**
- Modify: `crates/groupware/src/calendar/mod.rs:114-115` (the flag)
- Modify: `crates/groupware/src/calendar/seal/tree.rs:20` (imports), `:26-27` (`WRAP_MK` doc, `WRAP_PK`), after `:167` (`pk` envelopes)
- Create: `crates/groupware/src/calendar/seal/item.rs`
- Modify: `crates/groupware/src/calendar/seal/mod.rs`
- Test: `crates/groupware/src/calendar/seal/item.rs` (tests module)

**Interfaces:**
- Consumes:
  - Task 1: `pk_wrap`, `pk_unwrap`, `AAD_PK_DEK`, `PUBLIC_KEY_LEN`.
  - Task 2: `TokenPairs`, `TOKEN_LEN`, `seal_extra`, `open_extra`.
  - Existing: `seal_tree`, `unseal_tree`, `seal_key_envelope`, `open_key_envelope`, `has_stray_carriers`.
- Produces (Task 6, Task 8 and plans 9, 10 use these), all in `groupware::calendar::seal`:
  - `EVENT_NOTIFICATION_ZA_PENDING: u16 = 1 << 2` (in `groupware::calendar`).
  - `ITIP_SCOPE`, `RSVP_TOKEN_PROP`, `AAD_ITIP_KEY`.
  - `pub struct PendingCarriers { pub pair: Option<TokenPairs>, pub reply_token: Option<Zeroizing<[u8; 16]>> }`.
  - `seal_pending_item(ical: &mut ICalendar, recipient_id: u32, recipient_public: &[u8; 32], pair: Option<&TokenPairs>, reply_token: Option<&[u8; 16]>) -> Result<(), SealError>`. Plan 9's fan-out calls it.
  - `open_pending_item(ical: &mut ICalendar, recipient_id: u32, private_key: &Secret) -> Result<PendingCarriers, SealError>`. Plan 10's drain calls it; it is the only opener of a `pk` wrap.
  - `seal_drained_item(ical: &mut ICalendar, keys: &SessionKeys, owner_id: u32) -> Result<(), SealError>`. Plan 10's drain calls it.
  - `open_drained_item(ical: &mut ICalendar, keys: &SessionKeys, owner_id: u32) -> Result<(), SealError>`.
  - `unseal_item_archive(stored: &Archive<AlignedBytes>, keys: &SessionKeys, account_id: u32) -> Result<Archive<AlignedBytes>, SealError>`. Task 6's view calls it.
  - `item_is_sealed(&ICalendar) -> bool`.
  - `archived_item_is_pending(&ArchivedCalendarEventNotification) -> bool`.

Background:
- An item's tree is sealed under the release 1 policy with a fresh DEK and the scope `itip`, in both states (spec 5.3 step 2, spec 9).
- The pending envelope is the `pk` wrap to the recipient, and it binds the recipient and the UID: associated data `za/v1|pk-dek|<recipient>|<uid>`, release 1's event-key shape. The drained envelope is the same shape under the owner's EWK, with purpose `itip-key`. A pending item's `X-ZA-EXTRA` binds the recipient and the UID as well (`za/v1|itip-extra|<recipient>|<uid>`), as a release 1 event's extra bundle does. An item whose sealed parts are moved onto another UID's record therefore does not open (fix round 1, settling spec 3.1 against 5.3 and 9).
- A pending REQUEST or CANCEL carries the recipient's one pair in a sealed `X-ZA-EXTRA`. A pending REPLY carries the replier's token as the visible `X-ZA-RSVP-TOKEN`. A drained item carries neither.
- The pending flag lives here, not in Task 6, because `archived_item_is_pending` needs it.
- `seal_pending_item` builds the envelope before touching the tree, so a refused public key leaves the caller's tree unchanged.

- [ ] **Step 1: The flag and the tests**

In `crates/groupware/src/calendar/mod.rs`, after `EVENT_NOTIFICATION_IS_CHANGE` (`:115`):

```rust
/// Invitations spec 9: an inbox item still sealed to the recipient's public
/// key. Set by the fan-out (plan 9), cleared by the drain (plan 10); hidden
/// from clients meanwhile (spec 6.6).
pub const EVENT_NOTIFICATION_ZA_PENDING: u16 = 1 << 2;
```

Create `crates/groupware/src/calendar/seal/item.rs` with the SPDX header and only this tests module. Add `pub mod item;` to `seal/mod.rs` after `pub mod event;`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        CalendarEvent, CalendarEventData,
        seal::{
            event::{seal_event, unseal_event},
            tree::SEALED_PROP,
        },
    };
    use calcard::{
        Entry, Parser,
        common::timezone::Tz,
        icalendar::{ICalendarEntry, ICalendarProperty, ICalendarValue},
    };
    use vault::keys::{WRAP_LEN, generate_keypair};

    const ITEM: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nX-WR-CALNAME:item-root-canary\r\nBEGIN:VEVENT\r\nUID:item-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSEQUENCE:0\r\nSUMMARY:item-summary-canary\r\nORGANIZER;CN=Org:mailto:org@example.com\r\nATTENDEE;CN=Att;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn tree() -> ICalendar {
        match Parser::new(ITEM).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn root(ical: &mut ICalendar) -> &mut Vec<ICalendarEntry> {
        &mut ical.components[0].entries
    }

    fn pair() -> TokenPairs {
        let mut pairs = TokenPairs::new();
        pairs.insert("att@example.com", [7; TOKEN_LEN]);
        pairs
    }

    fn keys() -> SessionKeys {
        SessionKeys::new(9, 1, Secret::random())
    }

    /// A pending item for account 9 and the private key that opens it.
    fn pending(pair: Option<&TokenPairs>, reply: Option<&[u8; TOKEN_LEN]>) -> (ICalendar, Secret) {
        let (public, private) = generate_keypair();
        let mut ical = tree();
        seal_pending_item(&mut ical, 9, &public, pair, reply).unwrap();
        (ical, private)
    }

    fn envelope_of(ical: &ICalendar) -> Vec<u8> {
        STANDARD
            .decode(entry_text(ical.components[0].entries.last().unwrap()).unwrap())
            .unwrap()
    }

    fn archive_of(item: CalendarEventNotification) -> Archive<AlignedBytes> {
        let bytes = Archiver::new(item).serialize().unwrap();
        <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes).unwrap()
    }

    #[test]
    fn pending_item_with_a_pair_round_trips() {
        let (sealed, private) = pending(Some(&pair()), None);
        let dump = sealed.to_string();
        for canary in ["item-root-canary", "item-summary-canary", "att@example.com", "org@example.com"] {
            assert!(!dump.contains(canary), "{canary} in {dump}");
        }
        let entries = &sealed.components[0].entries;
        let n = entries.len();
        assert!(is_carrier(&entries[n - 1], KEY_PROP));
        assert!(is_carrier(&entries[n - 2], EXTRA_PROP));
        assert!(is_carrier(&entries[n - 3], SEALED_PROP));
        let envelope = envelope_of(&sealed);
        assert_eq!(envelope.len(), 2 + PUBLIC_KEY_LEN + WRAP_LEN);
        assert_eq!(&envelope[..2], &[POLICY_VERSION, WRAP_PK]);
        let mut opened = sealed.clone();
        let carriers = open_pending_item(&mut opened, 9, &private).unwrap();
        assert_eq!(opened, tree());
        let pair = carriers.pair.expect("pair");
        assert_eq!(pair.len(), 1);
        assert!(pair.matches("att@example.com", &[7; TOKEN_LEN]));
        assert!(carriers.reply_token.is_none());
    }

    #[test]
    fn pending_reply_item_carries_its_token_visibly() {
        let token = [3u8; TOKEN_LEN];
        let (sealed, private) = pending(None, Some(&token));
        let entries = &sealed.components[0].entries;
        let n = entries.len();
        assert!(is_carrier(&entries[n - 2], RSVP_TOKEN_PROP));
        assert_eq!(
            entry_text(&entries[n - 2]),
            Some(URL_SAFE_NO_PAD.encode(token).as_str())
        );
        let mut opened = sealed.clone();
        let carriers = open_pending_item(&mut opened, 9, &private).unwrap();
        assert_eq!(opened, tree());
        assert_eq!(carriers.reply_token.as_deref(), Some(&token));
        assert!(carriers.pair.is_none());
        // Neither carrier: the root ends with its bundle, then the key.
        let (bare, private) = pending(None, None);
        let n = bare.components[0].entries.len();
        assert!(is_carrier(&bare.components[0].entries[n - 2], SEALED_PROP));
        let carriers = open_pending_item(&mut bare.clone(), 9, &private).unwrap();
        assert!(carriers.pair.is_none() && carriers.reply_token.is_none());
    }

    #[test]
    fn pending_item_opens_only_with_the_recipients_private_key() {
        let (sealed, private) = pending(Some(&pair()), None);
        let (_, other) = generate_keypair();
        assert_eq!(
            open_pending_item(&mut sealed.clone(), 9, &other).err(),
            Some(SealError::Aead)
        );
        assert_eq!(
            open_pending_item(&mut sealed.clone(), 10, &private).err(),
            Some(SealError::Aead)
        );
        // The master-key view refuses it: only the drain opens it (invariant 11).
        assert_eq!(
            open_drained_item(&mut sealed.clone(), &keys(), 9),
            Err(SealError::Structure("pending item"))
        );
    }

    #[test]
    fn pending_item_carriers_out_of_order_are_stray() {
        let (sealed, private) = pending(Some(&pair()), None);
        let n = sealed.components[0].entries.len();
        // [.., SEALED, EXTRA, KEY] -> [.., EXTRA, SEALED, KEY].
        let mut swapped = sealed.clone();
        root(&mut swapped).swap(n - 3, n - 2);
        // EXTRA moved into the VEVENT, ahead of that component's SEALED.
        let mut moved = sealed.clone();
        let extra = root(&mut moved).remove(n - 2);
        let vevent = &mut moved.components[1].entries;
        let at = vevent.len() - 1;
        vevent.insert(at, extra);
        // A reply token next to the pair: [.., SEALED, RSVP, EXTRA, KEY].
        let mut both = sealed.clone();
        root(&mut both).insert(
            n - 2,
            text_entry(RSVP_TOKEN_PROP, URL_SAFE_NO_PAD.encode([1u8; TOKEN_LEN])),
        );
        // A reply item's token moved into its VEVENT.
        let (reply, reply_private) = pending(None, Some(&[3; TOKEN_LEN]));
        let m = reply.components[0].entries.len();
        let mut reply_moved = reply.clone();
        let token = root(&mut reply_moved).remove(m - 2);
        reply_moved.components[1].entries.insert(0, token);
        for (name, mut item, key) in [
            ("swapped", swapped, &private),
            ("moved", moved, &private),
            ("both", both, &private),
            ("reply moved", reply_moved, &reply_private),
        ] {
            assert_eq!(
                open_pending_item(&mut item, 9, key).err(),
                Some(SealError::Structure("misplaced carrier")),
                "{name}"
            );
        }
        // The key envelope not last: not a sealed item.
        let mut key_not_last = sealed.clone();
        let key = root(&mut key_not_last).pop().unwrap();
        root(&mut key_not_last).insert(n - 2, key);
        assert_eq!(
            open_pending_item(&mut key_not_last, 9, &private).err(),
            Some(SealError::NotSealed)
        );
    }

    #[test]
    fn an_item_carries_one_pair_or_a_token_and_needs_a_usable_key() {
        let (public, _) = generate_keypair();
        let mut two = pair();
        two.insert("other@example.com", [8; TOKEN_LEN]);
        let mut ical = tree();
        assert_eq!(
            seal_pending_item(&mut ical, 9, &public, Some(&two), None),
            Err(SealError::Structure("an item carries one pair"))
        );
        assert_eq!(
            seal_pending_item(&mut ical, 9, &public, Some(&pair()), Some(&[1; TOKEN_LEN])),
            Err(SealError::Structure("a pair and a reply token"))
        );
        assert_eq!(
            seal_pending_item(&mut ical, 9, &[0u8; PUBLIC_KEY_LEN], None, None),
            Err(SealError::Structure("recipient public key"))
        );
        assert_eq!(ical, tree(), "a refused seal leaves the tree untouched");
    }

    #[test]
    fn drained_item_round_trips_under_the_owners_keys_only() {
        let keys = keys();
        let mut sealed = tree();
        seal_drained_item(&mut sealed, &keys, 9).unwrap();
        let entries = &sealed.components[0].entries;
        let n = entries.len();
        assert!(is_carrier(&entries[n - 1], KEY_PROP) && is_carrier(&entries[n - 2], SEALED_PROP));
        assert!(
            !entries
                .iter()
                .any(|e| is_carrier(e, EXTRA_PROP) || is_carrier(e, RSVP_TOKEN_PROP))
        );
        assert_eq!(envelope_of(&sealed)[1], 1, "wrap type mk");
        let mut opened = sealed.clone();
        open_drained_item(&mut opened, &keys, 9).unwrap();
        assert_eq!(opened, tree());
        assert_eq!(
            open_drained_item(&mut sealed.clone(), &SessionKeys::new(9, 1, Secret::random()), 9),
            Err(SealError::Aead)
        );
        assert_eq!(open_drained_item(&mut sealed.clone(), &keys, 10), Err(SealError::Aead));
        assert_eq!(
            seal_drained_item(&mut tree(), &SessionKeys::new(10, 1, Secret::random()), 9),
            Err(SealError::Structure("keys of another account"))
        );
    }

    #[test]
    fn drained_item_with_a_pending_carrier_is_refused() {
        let keys = keys();
        let mut sealed = tree();
        seal_drained_item(&mut sealed, &keys, 9).unwrap();
        let n = sealed.components[0].entries.len();
        for carrier in [RSVP_TOKEN_PROP, EXTRA_PROP] {
            let mut item = sealed.clone();
            root(&mut item).insert(n - 1, text_entry(carrier, "AA".into()));
            assert_eq!(
                open_drained_item(&mut item, &keys, 9),
                Err(SealError::Structure("misplaced carrier")),
                "{carrier}"
            );
        }
    }

    /// Rewrites the VEVENT's UID, as a store writer moving an item's sealed
    /// parts onto another UID's record would.
    fn with_uid(ical: &ICalendar, uid: &str) -> ICalendar {
        let mut moved = ical.clone();
        let entry = moved.components[1]
            .entries
            .iter_mut()
            .find(|e| e.name == ICalendarProperty::Uid)
            .expect("UID entry");
        entry.values = vec![ICalendarValue::Text(uid.to_string())];
        moved
    }

    #[test]
    fn an_item_moved_onto_another_uid_does_not_open() {
        // Pending: the envelope binds the recipient and the UID. The extra
        // bundle binds them too, but that binding is not separately
        // observable through `open_pending_item`, because the envelope fails
        // first; it is belt and braces.
        let (sealed, private) = pending(Some(&pair()), None);
        assert_eq!(
            open_pending_item(&mut with_uid(&sealed, "item-2"), 9, &private).err(),
            Some(SealError::Aead)
        );
        // Drained: the envelope binds the owner and the UID.
        let keys = keys();
        let mut drained = tree();
        seal_drained_item(&mut drained, &keys, 9).unwrap();
        assert_eq!(
            open_drained_item(&mut with_uid(&drained, "item-2"), &keys, 9),
            Err(SealError::Aead)
        );
        // Control: the unmoved items open with the same keys.
        assert!(open_pending_item(&mut sealed.clone(), 9, &private).is_ok());
        assert!(open_drained_item(&mut drained.clone(), &keys, 9).is_ok());
    }

    #[test]
    fn itip_scope_and_event_scope_do_not_mix() {
        // Bundles: the same DEK under the other scope does not open.
        let dek = Secret::random();
        let mut as_item = tree();
        seal_tree(&mut as_item, &dek, 9, ITIP_SCOPE);
        assert_eq!(unseal_tree(&mut as_item, &dek, 9, "item-1"), Err(SealError::Aead));
        let mut as_event = tree();
        seal_tree(&mut as_event, &dek, 9, "item-1");
        assert_eq!(unseal_tree(&mut as_event, &dek, 9, ITIP_SCOPE), Err(SealError::Aead));
        // Envelopes: a sealed event is not a drained item, nor the reverse.
        let keys = keys();
        let mut event = CalendarEvent {
            data: CalendarEventData::new(tree(), Tz::Floating, 100, &mut None),
            ..Default::default()
        };
        seal_event(&mut event, &keys, 9, None).unwrap();
        assert_eq!(
            open_drained_item(&mut event.data.event.clone(), &keys, 9),
            Err(SealError::Aead)
        );
        let mut drained = tree();
        seal_drained_item(&mut drained, &keys, 9).unwrap();
        let mut as_event = CalendarEvent {
            data: CalendarEventData::new(drained, Tz::Floating, 100, &mut None),
            ..Default::default()
        };
        assert_eq!(unseal_event(&mut as_event, &keys, 9), Err(SealError::Aead));
    }

    #[test]
    fn item_views_pending_drained_and_plaintext() {
        let keys = keys();
        // Pending (Review Focus 3): an unseal failure, never opened here.
        let (public, _) = generate_keypair();
        let mut ical = tree();
        seal_pending_item(&mut ical, 9, &public, None, None).unwrap();
        let stored = archive_of(CalendarEventNotification {
            event: ical,
            flags: EVENT_NOTIFICATION_ZA_PENDING,
            ..Default::default()
        });
        assert!(archived_item_is_pending(
            stored.unarchive::<CalendarEventNotification>().unwrap()
        ));
        assert_eq!(
            unseal_item_archive(&stored, &keys, 9).err(),
            Some(SealError::Structure("pending item"))
        );
        // Plaintext (Review Focus 4): passes through as stored.
        let plain = archive_of(CalendarEventNotification {
            event: tree(),
            ..Default::default()
        });
        assert!(!archived_item_is_pending(
            plain.unarchive::<CalendarEventNotification>().unwrap()
        ));
        let view = unseal_item_archive(&plain, &keys, 9).unwrap();
        assert_eq!(view.as_bytes(), plain.as_bytes());
        assert_eq!(view.version, plain.version);
        // Drained: a plaintext view that keeps the stored version.
        let mut ical = tree();
        seal_drained_item(&mut ical, &keys, 9).unwrap();
        let stored = archive_of(CalendarEventNotification {
            event: ical,
            ..Default::default()
        });
        assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("item-summary-canary"));
        let view = unseal_item_archive(&stored, &keys, 9).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(view.deserialize::<CalendarEventNotification>().unwrap().event, tree());
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p groupware seal::item 2>&1 | tail -10`
Expected: compile errors, `cannot find function 'seal_pending_item'` and the other unresolved names.

- [ ] **Step 3: `pk` envelopes in `tree.rs`**

Replace the vault import (`:20`) with:

```rust
use vault::keys::{
    PUBLIC_KEY_LEN, Secret, WRAP_LEN, aad, open, pk_unwrap, pk_wrap, seal, unwrap_key, wrap_key,
};
```

Replace `WRAP_MK` and its comment (`:26-27`) with:

```rust
/// Wrap type `mk` (spec 7.1): the DEK wrapped under the account's event
/// wrapping key.
const WRAP_MK: u8 = 1;
/// Wrap type `pk` (invitations spec 3.1): the DEK wrapped to a recipient's
/// X25519 public key. Opened only by the drain (invariant 11).
pub(super) const WRAP_PK: u8 = 2;
```

After `open_key_envelope` (`:154-167`):

```rust
/// A `pk` key envelope: `policy version || 0x02 || E (32) || wrapped DEK
/// (72)` (invitations spec 3.1, 9).
pub(super) fn seal_pk_envelope(
    dek: &Secret,
    recipient_public: &[u8; PUBLIC_KEY_LEN],
    aad: &[u8],
) -> Result<Vec<u8>, SealError> {
    let (ephemeral, wrapped) = pk_wrap(dek, recipient_public, aad)
        .map_err(|_| SealError::Structure("recipient public key"))?;
    let mut envelope = Vec::with_capacity(2 + PUBLIC_KEY_LEN + WRAP_LEN);
    envelope.extend_from_slice(&[POLICY_VERSION, WRAP_PK]);
    envelope.extend_from_slice(&ephemeral);
    envelope.extend_from_slice(&wrapped);
    Ok(envelope)
}

/// Opens a `seal_pk_envelope` with the recipient's private key.
pub(super) fn open_pk_envelope(
    envelope: &[u8],
    private_key: &Secret,
    aad: &[u8],
) -> Result<Secret, SealError> {
    match envelope {
        [POLICY_VERSION, WRAP_PK, rest @ ..] if rest.len() == PUBLIC_KEY_LEN + WRAP_LEN => {
            let (ephemeral, wrapped) = rest.split_at(PUBLIC_KEY_LEN);
            let ephemeral: [u8; PUBLIC_KEY_LEN] =
                ephemeral.try_into().map_err(|_| SealError::Format)?;
            pk_unwrap(&ephemeral, wrapped, private_key, aad).map_err(|_| SealError::Aead)
        }
        [version, ..] if *version != POLICY_VERSION => Err(SealError::Policy(*version)),
        _ => Err(SealError::Format),
    }
}
```

- [ ] **Step 4: Implement `item.rs`**

Prepend to `item.rs`, after the SPDX header:

```rust
//! Sealing of scheduling inbox items (invitations spec 3.1, 5.3, 6.4 step 5,
//! 6.6, 9).
//!
//! An item's tree is sealed by `seal_tree` under the scope `itip` with a
//! fresh DEK, in both of its states:
//! - pending: the DEK is wrapped to the recipient's public key (`pk`), with
//!   associated data naming the recipient and the UID. The root may carry
//!   the recipient's token pair in a sealed `X-ZA-EXTRA` bound to the same
//!   recipient and UID (a REQUEST or CANCEL), or the replier's token as the
//!   visible `X-ZA-RSVP-TOKEN` (a REPLY). Written by the fan-out (plan 9),
//!   opened only by the drain (plan 10, invariant 11).
//! - drained: resealed under the owner's event wrapping key (`mk`), with
//!   neither carrier. Written by the drain, read by the inbox views.
//!
//! Carrier order on the root, as its last entries: `X-ZA-SEALED` (if any),
//! then `X-ZA-RSVP-TOKEN` or `X-ZA-EXTRA` (pending only, at most one), then
//! `X-ZA-KEY`. Any of them anywhere else is a stray carrier.

use super::{
    event::{open_extra, seal_extra},
    policy::POLICY_VERSION,
    token::{TOKEN_LEN, TokenPairs},
    tree::{
        EXTRA_PROP, KEY_PROP, SealError, WRAP_PK, entry_text, has_stray_carriers, is_carrier,
        open_key_envelope, open_pk_envelope, seal_key_envelope, seal_pk_envelope, seal_tree,
        text_entry, unseal_tree,
    },
};
use crate::calendar::{
    ArchivedCalendarEventNotification, CalendarEventNotification, EVENT_NOTIFICATION_ZA_PENDING,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use calcard::icalendar::{ICalendar, ICalendarComponentType};
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, Archiver},
};
use types::dead_property::DeadProperty;
use vault::{
    keys::{AAD_PK_DEK, PUBLIC_KEY_LEN, Secret, aad},
    session::SessionKeys,
};
use zeroize::Zeroizing;

/// Associated-data scope of an item's bundles, pending and drained (spec 9).
pub const ITIP_SCOPE: &str = "itip";
/// The replier's token on a pending reply item: visible, base64url without
/// padding (spec 3.3, 9).
pub const RSVP_TOKEN_PROP: &str = "X-ZA-RSVP-TOKEN";
/// Purpose of a drained item's key envelope (spec 9).
pub const AAD_ITIP_KEY: &str = "itip-key";
/// Purpose of a pending item's `X-ZA-EXTRA` bundle.
const AAD_ITIP_EXTRA: &str = "itip-extra";

/// What a pending item carries besides its tree. No `Debug`: tokens.
pub struct PendingCarriers {
    /// The recipient's own pair (a REQUEST or CANCEL).
    pub pair: Option<TokenPairs>,
    /// The replier's token (a REPLY).
    pub reply_token: Option<Zeroizing<[u8; TOKEN_LEN]>>,
}

fn uid_of(ical: &ICalendar) -> String {
    ical.uids().next().unwrap_or_default().to_string()
}

/// `za/v1|itip-key|<owner>|<uid>`: the event-key shape (spec 9).
fn drained_key_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad(AAD_ITIP_KEY, account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

/// `za/v1|pk-dek|<recipient>|<uid>`: a pending item's key envelope, in the
/// event-key shape (spec 9).
fn pending_key_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad(AAD_PK_DEK, account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

/// `za/v1|itip-extra|<recipient>|<uid>`: a pending item's `X-ZA-EXTRA`
/// bundle, bound like an event's (spec 9).
fn extra_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad(AAD_ITIP_EXTRA, account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

fn check_root(ical: &ICalendar) -> Result<(), SealError> {
    match ical.components.first() {
        Some(root) if root.component_type == ICalendarComponentType::VCalendar => Ok(()),
        Some(_) => Err(SealError::Structure("root is not VCALENDAR")),
        None => Err(SealError::Structure("empty tree")),
    }
}

/// True if the root's last entry is a key envelope (either state).
pub fn item_is_sealed(ical: &ICalendar) -> bool {
    ical.components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| is_carrier(e, KEY_PROP))
}

/// The pending bit (spec 9) of a stored item.
pub fn archived_item_is_pending(item: &ArchivedCalendarEventNotification) -> bool {
    item.flags.to_native() & EVENT_NOTIFICATION_ZA_PENDING != 0
}

/// Pops the root's key envelope and decodes it.
fn pop_envelope(ical: &mut ICalendar) -> Result<Vec<u8>, SealError> {
    if !item_is_sealed(ical) {
        return Err(SealError::NotSealed);
    }
    let key_entry = ical.components[0]
        .entries
        .pop()
        .ok_or(SealError::Structure("key envelope"))?;
    STANDARD
        .decode(entry_text(&key_entry).ok_or(SealError::Format)?)
        .map_err(|_| SealError::Decode)
}

/// Seals a plaintext item tree for `recipient_id` in its pending form (spec
/// 5.3 steps 1-3). `pair` is the recipient's own pair (a REQUEST or
/// CANCEL), `reply_token` the replier's token (a REPLY); never both. A
/// refused public key leaves the tree unchanged.
pub fn seal_pending_item(
    ical: &mut ICalendar,
    recipient_id: u32,
    recipient_public: &[u8; PUBLIC_KEY_LEN],
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
) -> Result<(), SealError> {
    check_root(ical)?;
    if pair.is_some() && reply_token.is_some() {
        return Err(SealError::Structure("a pair and a reply token"));
    }
    if pair.is_some_and(|pair| pair.len() != 1) {
        return Err(SealError::Structure("an item carries one pair"));
    }
    let uid = uid_of(ical);
    let dek = Secret::random();
    let envelope = seal_pk_envelope(
        &dek,
        recipient_public,
        &pending_key_aad(recipient_id, &uid),
    )?;
    seal_tree(ical, &dek, recipient_id, ITIP_SCOPE);
    let root = &mut ical.components[0].entries;
    if let Some(text) = pair.and_then(|pair| {
        seal_extra(
            &dek,
            &extra_aad(recipient_id, &uid),
            None,
            DeadProperty::default(),
            Some(pair),
        )
    }) {
        root.push(text_entry(EXTRA_PROP, text));
    }
    if let Some(token) = reply_token {
        root.push(text_entry(RSVP_TOKEN_PROP, URL_SAFE_NO_PAD.encode(token)));
    }
    root.push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

/// Opens a pending item in place with the recipient's private key (the
/// drain, spec 6.4 step 1) and returns what it carries. On `Err` the tree
/// must be discarded.
pub fn open_pending_item(
    ical: &mut ICalendar,
    recipient_id: u32,
    private_key: &Secret,
) -> Result<PendingCarriers, SealError> {
    check_root(ical)?;
    // UID is a visible property of every component; the root pops below do
    // not change it.
    let uid = uid_of(ical);
    let envelope = pop_envelope(ical)?;
    let dek = open_pk_envelope(&envelope, private_key, &pending_key_aad(recipient_id, &uid))?;
    let root = &mut ical.components[0].entries;
    let extra = root.pop_if(|e| is_carrier(e, EXTRA_PROP));
    let reply = if extra.is_none() {
        root.pop_if(|e| is_carrier(e, RSVP_TOKEN_PROP))
    } else {
        None
    };
    if has_stray_carriers(ical) {
        return Err(SealError::Structure("misplaced carrier"));
    }
    let pair = match extra {
        Some(carrier) => {
            open_extra(
                &dek,
                &extra_aad(recipient_id, &uid),
                entry_text(&carrier).ok_or(SealError::Format)?,
            )?
            .pairs
        }
        None => None,
    };
    let reply_token = match reply {
        Some(carrier) => {
            let raw = Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(entry_text(&carrier).ok_or(SealError::Format)?)
                    .map_err(|_| SealError::Decode)?,
            );
            Some(Zeroizing::new(
                <[u8; TOKEN_LEN]>::try_from(raw.as_slice()).map_err(|_| SealError::Format)?,
            ))
        }
        None => None,
    };
    unseal_tree(ical, &dek, recipient_id, ITIP_SCOPE)?;
    Ok(PendingCarriers { pair, reply_token })
}

/// Reseals a plaintext item tree under the owner's keys: the drained form
/// (spec 6.4 step 5), with neither `X-ZA-EXTRA` nor `X-ZA-RSVP-TOKEN`.
pub fn seal_drained_item(
    ical: &mut ICalendar,
    keys: &SessionKeys,
    owner_id: u32,
) -> Result<(), SealError> {
    check_root(ical)?;
    if keys.account_id != owner_id {
        return Err(SealError::Structure("keys of another account"));
    }
    let uid = uid_of(ical);
    let dek = Secret::random();
    seal_tree(ical, &dek, owner_id, ITIP_SCOPE);
    let envelope = seal_key_envelope(&dek, keys.ewk(), &drained_key_aad(owner_id, &uid));
    ical.components[0]
        .entries
        .push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

/// Opens a drained item in place (the inbox views). A pending (`pk`) item
/// is `Structure("pending item")`: only the drain opens it (spec 10). A tree
/// without a key envelope is plaintext and stays unchanged. On `Err` the
/// tree must be discarded.
pub fn open_drained_item(
    ical: &mut ICalendar,
    keys: &SessionKeys,
    owner_id: u32,
) -> Result<(), SealError> {
    if !item_is_sealed(ical) {
        return Ok(());
    }
    let envelope = pop_envelope(ical)?;
    if matches!(envelope.as_slice(), [POLICY_VERSION, WRAP_PK, ..]) {
        return Err(SealError::Structure("pending item"));
    }
    let uid = uid_of(ical);
    let dek = open_key_envelope(&envelope, keys.ewk(), &drained_key_aad(owner_id, &uid))?;
    if has_stray_carriers(ical) {
        return Err(SealError::Structure("misplaced carrier"));
    }
    unseal_tree(ical, &dek, owner_id, ITIP_SCOPE)
}

/// Spec 6.6: the read view of a stored item, with `unseal_event_archive`'s
/// contract: `version` copied from the stored archive, read-only, never
/// written back. A plaintext item yields a copy of `stored`.
pub fn unseal_item_archive(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Archive<AlignedBytes>, SealError> {
    let mut item = stored
        .deserialize::<CalendarEventNotification>()
        .map_err(|_| SealError::Format)?;
    if !item_is_sealed(&item.event) {
        return Ok(stored.clone());
    }
    open_drained_item(&mut item.event, keys, account_id)?;
    let bytes = Archiver::new(item)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}
```

In `seal/mod.rs`, add the item re-export:

```rust
pub use item::{
    AAD_ITIP_KEY, ITIP_SCOPE, PendingCarriers, RSVP_TOKEN_PROP, archived_item_is_pending,
    item_is_sealed, open_drained_item, open_pending_item, seal_drained_item, seal_pending_item,
    unseal_item_archive,
};
```

- [ ] **Step 5: Run to see them pass**

Run: `cargo test -p groupware 2>&1 | tail -5`
Expected: PASS, 69 tests (Task 2's 59 plus 10 item tests).

- [ ] **Step 6: Red run, carriers out of order (M20)**

In `crates/groupware/src/calendar/seal/item.rs`, `open_pending_item`, delete the three-line `if has_stray_carriers(ical) { .. }` block. Run `cargo test -p groupware seal::item`.
Expected: FAIL in `pending_item_carriers_out_of_order_are_stray` with `left: None` (or `Some(Aead)`), `right: Some(Structure("misplaced carrier"))`, case `swapped`.

Restore the block exactly as Step 4 shows it; the file is new and uncommitted, so `git checkout` cannot restore it. Rerun `cargo test -p groupware seal::item` to green. Write the failure line into the task report.

- [ ] **Step 7: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p groupware -- --check
git add crates/groupware/src/calendar/mod.rs crates/groupware/src/calendar/seal/
git commit -m "Seal: inbox items, pending to the recipient's public key and drained under MK"
```

---

### Task 4: The private key in session keys

**Files:**
- Modify: `crates/vault/src/session.rs:7-46` (struct, constructor, accessors), tests `:48-63`
- Modify: `crates/common/src/auth/vault.rs:8-16` (imports), after `:318` (helper), `:394-425` (`impl Server`), tests `:441-835`

**Interfaces:**
- Consumes: `vault_read_store`, `is_vault_unusable`, `corrupt` (`common/src/auth/vault.rs`); `unwrap_key`, `aad`, `AAD_PRIVATE_KEY`, `KeyError` (`vault::keys`).
- Produces:
  - `SessionKeys::private_key(&self) -> Option<Result<&Secret, KeyError>>`: `None` until the first unwrap was attempted.
  - `SessionKeys::set_private_key(&self, outcome: Result<Secret, KeyError>) -> bool`: false if an outcome was already recorded.
  - `pub(crate) async fn za_open_private_key<'k>(store: &Store, keys: &'k SessionKeys) -> trc::Result<&'k Secret>`.
  - `Server::za_private_key<'k>(&self, keys: &'k SessionKeys) -> trc::Result<&'k Secret>`.
    - Plan 10's drain calls this once per pass.
    - Its error is already traced (once per session), so the drain skips without tracing again (spec 10).

Background:
- The setup endpoint wraps the private key as `wrap_key(&private, &mk, &aad(AAD_PRIVATE_KEY, account_id))` (`crates/http/src/api/vault.rs:878-880`). The wrapping key is MK itself, not EWK, and the purpose is `private-key`.
- A session is one key-cache entry, an `Arc<SessionKeys>` (`crates/vault/src/cache.rs`). The private key lives in that struct only and is zeroed with it (spec 3.2).
- Recovery and password changes rewrap MK and leave `private_key_wrap` valid (`tests/src/za/password.rs:172` asserts it).
- Spec 3.2 also has `SessionKeys` remember the inbox change id it last drained. That cursor belongs to the drain (spec 6.2) and is plan 10's; this task adds only the private key.
- Outcomes:

  | Case | Recorded? | What happens |
  |---|---|---|
  | Store error | No | Propagated; a later call retries (spec 6.5, "transient"). |
  | Wrap does not open | Yes | Traced once; every call of the session is an error. |
  | Missing or unusable record | Yes | Same as a wrap that does not open. |

- The trace uses the existing `corrupt` helper: `StoreEvent::DataCorruption` with a `zero-access:` details prefix and the account id only. No new event type.

- [ ] **Step 1: Write the failing tests**

In `crates/vault/src/session.rs`, append inside `mod tests`:

```rust
    use crate::keys::KeyError;

    #[test]
    fn private_key_is_recorded_once_and_never_printed() {
        let keys = SessionKeys::new(3, 11, Secret::from_bytes([9; 32]));
        assert!(keys.private_key().is_none(), "nothing before first use");
        assert!(keys.set_private_key(Ok(Secret::from_bytes([0xAB; 32]))));
        assert!(
            !keys.set_private_key(Err(KeyError::Aead)),
            "the first outcome stands"
        );
        assert_eq!(keys.private_key().unwrap().unwrap().as_bytes(), &[0xAB; 32]);
        assert_eq!(format!("{keys:?}"), "SessionKeys(account 3, generation 11)");
        let failed = SessionKeys::new(3, 11, Secret::random());
        assert!(failed.set_private_key(Err(KeyError::Aead)));
        assert_eq!(
            failed.private_key().map(|outcome| outcome.err()),
            Some(Some(KeyError::Aead))
        );
    }
```

In `crates/common/src/auth/vault.rs`, append inside `mod tests`:

```rust
    fn record_with_private_key(account_id: u32, mk: &Secret, private: &Secret) -> VaultRecord {
        let mut record = VaultRecord::pending(vec![], 0);
        record.state = VaultState::Active;
        record.private_key_wrap = wrap_key(private, mk, &aad(AAD_PRIVATE_KEY, account_id));
        record
    }

    #[tokio::test]
    async fn private_key_unwraps_from_the_record_once_per_session() {
        let store = EphemeralStore::open();
        let mk = Secret::random();
        let (public, private) = ::vault::keys::generate_keypair();
        let mut record = record_with_private_key(3, &mk, &private);
        record.public_key = public.to_vec();
        vault_write_store(&store, 3, &record, None).await.unwrap();

        let keys = SessionKeys::new(3, 1, mk.clone());
        assert!(keys.private_key().is_none());
        let key = za_open_private_key(&store, &keys).await.unwrap();
        assert_eq!(key.as_bytes(), private.as_bytes());
        // Kept in the session: no second record read (the record is now
        // unusable, and the call still succeeds).
        write_raw(&store, 3, Vec::new()).await;
        assert_eq!(
            za_open_private_key(&store, &keys).await.unwrap().as_bytes(),
            private.as_bytes()
        );
        // A new session reads the record again.
        assert!(
            za_open_private_key(&store, &SessionKeys::new(3, 1, mk.clone()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_private_key_wrap_that_does_not_open_is_an_error_and_no_key() {
        let store = EphemeralStore::open();
        let mk = Secret::random();
        let private = Secret::random();
        let mut record = record_with_private_key(3, &mk, &private);
        record.private_key_wrap[40] ^= 1;
        vault_write_store(&store, 3, &record, None).await.unwrap();

        let keys = SessionKeys::new(3, 1, mk.clone());
        let err = za_open_private_key(&store, &keys).await.err().unwrap();
        assert!(err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption)));
        assert!(format!("{err:?}").contains("zero-access:"), "{err:?}");
        assert!(matches!(keys.private_key(), Some(Err(_))), "no key is kept");

        // Recorded (and traced) once: repairing the record does not change
        // this session's outcome; a fresh session opens the repaired wrap.
        let read = vault_read_store(&store, 3).await.unwrap().unwrap();
        let mut repaired = record_with_private_key(3, &mk, &private);
        repaired.revision = read.record.revision + 1;
        vault_write_store(&store, 3, &repaired, Some(&read)).await.unwrap();
        assert!(za_open_private_key(&store, &keys).await.is_err());
        assert_eq!(
            za_open_private_key(&store, &SessionKeys::new(3, 1, mk.clone()))
                .await
                .unwrap()
                .as_bytes(),
            private.as_bytes()
        );

        // A missing record, and a wrap bound to another account.
        assert!(
            za_open_private_key(&store, &SessionKeys::new(4, 1, mk.clone()))
                .await
                .is_err()
        );
        let moved = record_with_private_key(3, &mk, &private);
        vault_write_store(&store, 5, &moved, None).await.unwrap();
        assert!(
            za_open_private_key(&store, &SessionKeys::new(5, 1, mk.clone()))
                .await
                .is_err()
        );
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p vault session 2>&1 | tail -5`, then `cargo test -p common auth::vault 2>&1 | tail -10`.
Expected: compile errors, `no method named 'private_key'` and `cannot find function 'za_open_private_key'`.

- [ ] **Step 3: Implement the session field**

In `crates/vault/src/session.rs`, replace `:7-36` with:

```rust
use crate::keys::{KeyError, Secret, derive_event_key};
use std::sync::OnceLock;

/// Keys resident for one request (spec 5, "in-flight copies"). Cloned by
/// `Arc`; the secrets are zeroed when the last reference drops.
pub struct SessionKeys {
    pub account_id: u32,
    pub generation: u64,
    mk: Secret,
    ewk: Secret,
    /// Invitations spec 3.2: the X25519 private key, unwrapped from the
    /// vault record on first use. Holds the outcome, so a wrap that does not
    /// open is traced once per session.
    private_key: OnceLock<Result<Secret, KeyError>>,
}

impl SessionKeys {
    pub fn new(account_id: u32, generation: u64, mk: Secret) -> Self {
        let ewk = derive_event_key(&mk);
        SessionKeys {
            account_id,
            generation,
            mk,
            ewk,
            private_key: OnceLock::new(),
        }
    }

    pub fn mk(&self) -> &Secret {
        &self.mk
    }

    pub fn ewk(&self) -> &Secret {
        &self.ewk
    }

    /// `None` until the first unwrap was attempted.
    pub fn private_key(&self) -> Option<Result<&Secret, KeyError>> {
        self.private_key
            .get()
            .map(|outcome| outcome.as_ref().map_err(|err| *err))
    }

    /// Records the unwrap outcome; `false` if one was already recorded (the
    /// first stands).
    pub fn set_private_key(&self, outcome: Result<Secret, KeyError>) -> bool {
        self.private_key.set(outcome).is_ok()
    }
}
```

The `Debug` impl (`:38-46`) is unchanged. It prints the account and the generation only.

- [ ] **Step 4: Implement the server helper**

In `crates/common/src/auth/vault.rs`, extend the `::vault::keys` import (`:10-13`) with `AAD_PRIVATE_KEY, KeyError`.

After `fn corrupt` (`:312-318`):

```rust
/// Invitations spec 3.2: the session's X25519 private key, unwrapped from
/// `private_key_wrap` under MK on first use and kept only in `keys`, which
/// zeroes it on drop. The outcome is recorded:
/// - a wrap that does not open, or a missing or unusable record, is traced
///   once per session (account id and a `zero-access:` description only) and
///   is an error on every call (spec 10);
/// - a store error is not recorded, so a later call retries.
pub(crate) async fn za_open_private_key<'k>(
    store: &Store,
    keys: &'k SessionKeys,
) -> trc::Result<&'k Secret> {
    if keys.private_key().is_none() {
        let account_id = keys.account_id;
        let outcome = match vault_read_store(store, account_id).await {
            Ok(Some(read)) => unwrap_key(
                &read.record.private_key_wrap,
                keys.mk(),
                &aad(AAD_PRIVATE_KEY, account_id),
            ),
            Ok(None) => Err(KeyError::Aead),
            Err(err) if is_vault_unusable(&err) => Err(KeyError::Aead),
            Err(err) => return Err(err),
        };
        let failed = outcome.is_err();
        if keys.set_private_key(outcome) && failed {
            trc::error!(private_key_unavailable(account_id));
        }
    }
    match keys.private_key() {
        Some(Ok(key)) => Ok(key),
        _ => Err(private_key_unavailable(keys.account_id)),
    }
}

fn private_key_unavailable(account_id: u32) -> trc::Error {
    corrupt(account_id, "zero-access: session private key does not open")
}
```

In the second `impl Server` block (`:394-425`), add:

```rust
    /// Invitations spec 3.2: see `za_open_private_key`. The drain (plan 10)
    /// is the caller; an error was already traced for the session.
    pub async fn za_private_key<'k>(&self, keys: &'k SessionKeys) -> trc::Result<&'k Secret> {
        za_open_private_key(self.store(), keys).await
    }
```

- [ ] **Step 5: Run to see them pass**

Run: `cargo test -p vault 2>&1 | tail -3` (Task 1's 29 plus 1), then `cargo test -p common auth::vault 2>&1 | tail -3`.
Expected: both PASS.

Run: `cargo check -p stalwart --no-default-features --features rocks 2>&1 | grep -E "crates/(vault|common/src/auth)/" || echo clean`
Expected: `clean`.

- [ ] **Step 6: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p vault -- --check
cargo fmt -p common -- --check
git add crates/vault/src/session.rs crates/common/src/auth/vault.rs
git commit -m "Session keys: the private key, unwrapped from the vault record once per session"
```

---

### Task 5: Every rewrite carries the pairs

**Files:**
- Modify: `crates/dav/src/calendar/update.rs:35` (import), `:333-336` (update branch seal site)
- Modify: `crates/dav/src/calendar/proppatch.rs:31` (import), `:252-256` (event seal site)
- Modify: `tests/src/utils/za.rs:7-22` (imports), `:282-345` (`plant_event` split, `plant_sealed_event`, `za_unlock`)
- Create: `tests/src/za/pairs.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod pairs;`, and `pairs::test` after `websocket::test`, `:107`)

**Interfaces:**
- Consumes: Task 2's `seal_event(.., pairs)`, `event_token_pairs`, `TokenPairs`; `raw_event` (`tests/src/za/dav_seal.rs:39`); `Server::za_verify_password` -> `ZaVerification::Valid(Arc<SessionKeys>)` (`crates/common/src/auth/vault.rs:397-410`).
- Produces, used by Tasks 6, 8 and later plans' tests:
  - `pub async fn za_unlock(test: &TestServer, account_id: u32, password: &str) -> Arc<SessionKeys>`
  - `pub async fn plant_sealed_event(test: &TestServer, account_id: u32, calendar: &str, name: &str, ical: &str, password: &str, pairs: &TokenPairs) -> u32`
  - `pairs::test(test: &mut TestServer)`
  - The update and PROPPATCH seal sites now carry the stored pairs. Plan 9 replaces the create branch's `None` with the organizer's pairs; COPY and MOVE keep stored bytes and are untouched.

Background:
- Spec 3.3: `CalendarEvent` has no field for pairs. The unsealed view the client edits (`za_event_view`, `update.rs:160`; `unseal_event`, `proppatch.rs:231`) restores only the display name and dead properties.
- So each rewrite reads the stored pairs with `event_token_pairs` on the stored archive and passes them to `seal_event`.
- The update branch validates that the UID is unchanged (`update.rs:205-210`), so the stored pairs belong to the same UID.
- The view's unseal runs first, so a tampered bundle still answers 500 before `event_token_pairs` is reached.
- An alarm-only PUT and a PROPPATCH build no iTIP message; a key account resolves to `ItipSendStatus::KeyAccount`. They are the two rewrites the spec names (section 11).

- [ ] **Step 1: Test helpers**

In `tests/src/utils/za.rs`, add imports:
- `common::auth::vault::ZaVerification`
- `groupware::calendar::seal::{TokenPairs, seal_event}`
- `vault::session::SessionKeys`

`Arc` is already imported.

Replace `plant_event` (`:282-345`) with:

```rust
/// The account's session keys, unlocked from `password` by the product's own
/// login verification (`za_verify_password`: Argon2, verifier, MK unwrap), as
/// a CalDAV login unlocks them.
pub async fn za_unlock(test: &TestServer, account_id: u32, password: &str) -> Arc<SessionKeys> {
    match test
        .server
        .za_verify_password(account_id, password, None)
        .await
        .unwrap()
    {
        ZaVerification::Valid(keys) => keys,
        _ => panic!("the password does not unlock account {account_id}"),
    }
}

/// Writes `ical` into the store as the event `name` in the account's
/// calendar `calendar` (a slug such as "default"), bypassing the DAV
/// handlers and so the seal. Such an event is what a legacy plaintext event
/// (written before the account held keys) looks like; tests use it to reach
/// code that sealing otherwise hides. Schedules the event's next email alarm
/// the way a PUT does. Returns the document id.
pub async fn plant_event(
    test: &TestServer,
    account_id: u32,
    calendar: &str,
    name: &str,
    ical: &str,
    schedule_tag: Option<u32>,
) -> u32 {
    insert_event(test, account_id, calendar, name, ical, schedule_tag, None).await
}

/// `plant_event`, sealed under the account's keys (unlocked from
/// `password`) with `pairs` in a format 2 `X-ZA-EXTRA` bundle: the stored
/// form of a key organizer's event once the fan-out (plan 9) hands out
/// tokens. Returns the document id.
pub async fn plant_sealed_event(
    test: &TestServer,
    account_id: u32,
    calendar: &str,
    name: &str,
    ical: &str,
    password: &str,
    pairs: &TokenPairs,
) -> u32 {
    let keys = za_unlock(test, account_id, password).await;
    insert_event(test, account_id, calendar, name, ical, None, Some((&*keys, pairs))).await
}

async fn insert_event(
    test: &TestServer,
    account_id: u32,
    calendar: &str,
    name: &str,
    ical: &str,
    schedule_tag: Option<u32>,
    seal: Option<(&SessionKeys, &TokenPairs)>,
) -> u32 {
    let calendar_id = test
        .server
        .fetch_dav_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(calendar)
        .unwrap_or_else(|| panic!("calendar {calendar} not found"))
        .document_id();
    let size = ical.len() as u32;
    let ical = match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    };
    let mut next_alarm = None;
    let mut event = CalendarEvent {
        names: vec![common::DavName {
            name: name.into(),
            parent_id: calendar_id,
        }],
        data: CalendarEventData::new(ical, Tz::Floating, 100, &mut next_alarm),
        size,
        schedule_tag,
        ..Default::default()
    };
    if let Some((keys, pairs)) = seal {
        seal_event(&mut event, keys, account_id, Some(pairs)).unwrap();
    }
    let account_info = test.server.account_info(account_id).await.unwrap();
    let document_id = test
        .server
        .store()
        .assign_document_ids(account_id, Collection::CalendarEvent, 1)
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    event
        .insert(
            account_info.account_tenant_ids(),
            account_id,
            document_id,
            next_alarm,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    // A direct store write neither wakes the task manager nor refreshes the
    // DAV resource cache the way the DAV handlers do.
    test.server.notify_task_queue();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::DavResources(account_id)])
        .await;
    document_id
}
```

- [ ] **Step 2: The pairs test**

Create `tests/src/za/pairs.rs` with the SPDX header and:

```rust
//! Invitations spec 3.3, "Every rewrite carries the pairs": a key account's
//! own PUT and PROPPATCH keep the token pairs of the stored `X-ZA-EXTRA`
//! bundle, which the unsealed view a client edits cannot show.

use super::{STRONG, dav_seal::raw_event};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{plant_event, plant_sealed_event, za_unlock},
};
use groupware::calendar::{
    CalendarEvent,
    seal::{TOKEN_LEN, TokenPairs, archived_event_is_sealed, event_token_pairs},
};
use hyper::StatusCode;
use vault::session::SessionKeys;

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const PAIR_A: (&str, [u8; TOKEN_LEN]) = ("pair-a@example.com", *b"pairs-token-a-01");
const PAIR_B: (&str, [u8; TOKEN_LEN]) = ("pair-b@example.com", *b"pairs-token-b-02");

/// A key organizer's event with two attendees.
const EVENT: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
    "UID:za-pairs-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990101T100000Z\r\n",
    "DTEND:20990101T110000Z\r\nSEQUENCE:0\r\nSUMMARY:pairs-canary\r\n",
    "ORGANIZER:mailto:key1@example.com\r\n",
    "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:pair-a@example.com\r\n",
    "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:pair-b@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);

/// The pairs in the stored event's bundle, opened as the product opens them.
async fn stored_pairs(
    test: &TestServer,
    account_id: u32,
    keys: &SessionKeys,
    path: &str,
) -> Option<TokenPairs> {
    let (archive, _) = raw_event(test, account_id, path).await;
    event_token_pairs(&archive, keys, account_id).unwrap()
}

fn assert_both(pairs: Option<TokenPairs>, after: &str) {
    let pairs = pairs.unwrap_or_else(|| panic!("no token pairs after {after}"));
    assert_eq!(pairs.len(), 2, "pairs after {after}");
    for (address, token) in [PAIR_A, PAIR_B] {
        assert!(
            pairs.matches(address, &token),
            "{address} lost its token after {after}"
        );
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access token pair carry-forward tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let keys = za_unlock(test, id, STRONG).await;
    let path = "/dav/cal/key1%40example.com/default/pairs.ics";
    let mut pairs = TokenPairs::new();
    for (address, token) in [PAIR_A, PAIR_B] {
        pairs.insert(address, token);
    }
    plant_sealed_event(test, id, "default", "pairs.ics", EVENT, STRONG, &pairs).await;
    assert_both(
        stored_pairs(test, id, &keys, "default/pairs.ics").await,
        "planting",
    );
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("pairs-canary") && !body.contains("X-ZA-"), "{body}");

    // An alarm-only PUT builds no iTIP message; the bundle is rebuilt from
    // the view's fields and the stored pairs.
    let with_alarm = EVENT.replace(
        "END:VEVENT",
        "BEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nDESCRIPTION:pairs-alarm\r\nEND:VALARM\r\nEND:VEVENT",
    );
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], with_alarm)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_both(
        stored_pairs(test, id, &keys, "default/pairs.ics").await,
        "an alarm-only PUT",
    );
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("pairs-alarm") && !body.contains("X-ZA-"), "{body}");

    // A PROPPATCH of the display name rewrites the bundle the same way.
    client
        .proppatch(path, [("D:displayname", "pairs-name")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_both(
        stored_pairs(test, id, &keys, "default/pairs.ics").await,
        "a PROPPATCH",
    );
    client
        .propfind(path, ["D:displayname"])
        .await
        .properties(path)
        .get("D:displayname")
        .with_values(["pairs-name"]);

    // Review Focus 5: events without pairs take the same rewrites. A release
    // 1 event (its PROPPATCH writes a format 1 bundle) keeps its display name
    // through a PUT; a legacy plaintext event is sealed by its PUT. Neither
    // gains pairs.
    let r1 = "/dav/cal/key1%40example.com/default/pairs-r1.ics";
    let r1_body = EVENT.replace("za-pairs-1", "za-pairs-r1");
    client
        .request_with_headers("PUT", r1, [CONTENT_TYPE], r1_body.clone())
        .await
        .with_status(StatusCode::CREATED);
    client
        .proppatch(r1, [("D:displayname", "pairs-r1-name")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    client
        .request_with_headers(
            "PUT",
            r1,
            [CONTENT_TYPE],
            r1_body.replace("pairs-canary", "pairs-r1-changed"),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        stored_pairs(test, id, &keys, "default/pairs-r1.ics")
            .await
            .is_none()
    );
    client
        .propfind(r1, ["D:displayname"])
        .await
        .properties(r1)
        .get("D:displayname")
        .with_values(["pairs-r1-name"]);

    let legacy = "/dav/cal/key1%40example.com/default/pairs-legacy.ics";
    let legacy_body = EVENT.replace("za-pairs-1", "za-pairs-legacy");
    plant_event(test, id, "default", "pairs-legacy.ics", &legacy_body, None).await;
    client
        .request_with_headers(
            "PUT",
            legacy,
            [CONTENT_TYPE],
            legacy_body.replace("pairs-canary", "pairs-legacy-changed"),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (archive, _) = raw_event(test, id, "default/pairs-legacy.ics").await;
    assert!(archived_event_is_sealed(
        archive.unarchive::<CalendarEvent>().unwrap()
    ));
    assert!(event_token_pairs(&archive, &keys, id).unwrap().is_none());

    test.wait_for_tasks().await;
    for path in [path, r1, legacy] {
        client
            .request("DELETE", path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}
```

In `tests/src/za/mod.rs`, add `pub mod pairs;` after `pub mod leak;`, and `pairs::test(&mut test).await;` right after `websocket::test(&mut test).await;` (`:107`).

- [ ] **Step 3: Run to see it fail**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture 2>&1 | grep -E "panicked|token pairs|test result"`
Expected: FAIL with `no token pairs after an alarm-only PUT`: both rewrite sites still pass `None` (Task 2).

- [ ] **Step 4: Carry the stored pairs at both sites**

`crates/dav/src/calendar/update.rs`:
- In the import (`:35`), add `event_token_pairs` to the seal list: `seal::{event_token_pairs, seal_error, seal_event},`.
- Replace `:333-336` (the update branch) with:

```rust
            if let Some(keys) = &za_keys {
                // Invitations spec 3.3: every rewrite carries the stored pairs.
                let pairs = event_token_pairs(&event_, keys, account_id)
                    .map_err(|err| seal_error(err, account_id, document_id))?;
                seal_event(&mut new_event, keys, account_id, pairs.as_ref())
                    .map_err(|err| seal_error(err, account_id, document_id))?;
            }
```

The create branch (`:493-496`) keeps `None`.

`crates/dav/src/calendar/proppatch.rs`:
- In the import (`:31`), add `event_token_pairs`.
- Replace the seal block inside `if is_success` (`:253-256`) with:

```rust
                if let Some(keys) = &za_keys {
                    // Invitations spec 3.3: every rewrite carries the stored pairs.
                    let pairs = event_token_pairs(&archive, keys, account_id)
                        .map_err(|err| seal_error(err, account_id, document_id))?;
                    seal_event(&mut new_event, keys, account_id, pairs.as_ref())
                        .map_err(|err| seal_error(err, account_id, document_id))?;
                }
```

- [ ] **Step 5: Run green**

Run the za suite (Global Constraints). Expected: PASS, including `pairs::test`.

Fallback: if a PUT in the Review Focus 5 block answers 412 instead of 201 or 204, an earlier module left an event with that UID in key1's default calendar. Change the UID suffix (`za-pairs-r1`, `za-pairs-legacy`) and record it in the task report.

- [ ] **Step 6: Red run, the PROPPATCH site (M19)**

In `crates/dav/src/calendar/proppatch.rs`, change `pairs.as_ref()` in the new block to `None`. Run the za suite.
Expected: FAIL with `no token pairs after a PROPPATCH`. The PUT step passes, so the two sites are tested separately; M18, the update site, is Step 3's failure and Task 9 repeats it on its own.

Change it back to `pairs.as_ref()`, and confirm with `git diff -- crates/dav/src/calendar/proppatch.rs` that exactly Step 4's change remains. Write the failure line into the task report.

- [ ] **Step 7: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p dav -- --check
cargo fmt -p tests -- --check
git add crates/dav/src/calendar/update.rs crates/dav/src/calendar/proppatch.rs tests/src/utils/za.rs tests/src/za/pairs.rs tests/src/za/mod.rs
git commit -m "Every rewrite of a key account's event carries its token pairs"
```

---

### Task 6: The inbox as clients see it: pending items hidden, drained items unsealed

**Files:**
- Modify: `crates/dav/src/common/za.rs:9` (imports), `:157-180` (`za_archive_view` arm), new `za_item_view` and `za_item_hidden` after `za_calendar_view` (`:142-155`)
- Modify: `crates/dav/src/common/propfind.rs:24` (import), `:458` (the skip, after `stored_ = archive;`)
- Modify: `crates/dav/src/calendar/scheduling.rs:7-15` (imports), `:116-118` (GET)
- Modify: `tests/src/utils/za.rs` (imports; `PlantedItem`, `plant_inbox_item`, `rewrite_inbox_item`)
- Create: `tests/src/za/inbox.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod inbox;`, and `inbox::test` after `pairs::test`)

**Interfaces:**
- Consumes: Task 3's `unseal_item_archive`, `archived_item_is_pending`, `seal_pending_item`, `seal_drained_item`, `EVENT_NOTIFICATION_ZA_PENDING`; Task 5's `za_unlock`; `ZeroAccessGate::za_session_keys`, `is_key_account` (`za.rs`).
- Produces:
  - `pub(crate) fn za_item_view<'x>(stored: &'x Archive<AlignedBytes>, keys: Option<&Arc<SessionKeys>>, account_id: u32, document_id: u32) -> trc::Result<Cow<'x, Archive<AlignedBytes>>>`
  - `pub(crate) async fn za_item_hidden(server: &Server, account_id: u32, collection: Collection, stored: &Archive<AlignedBytes>) -> crate::Result<bool>`
  - Test helpers, used by Tasks 7, 8 and plans 9, 10:
    - `pub enum PlantedItem<'a> { Pending { pair: Option<(&'a str, [u8; 16])>, reply_token: Option<[u8; 16]> }, Drained { password: &'a str }, Plain { flags: u16 } }`
    - `pub async fn plant_inbox_item(test: &TestServer, account_id: u32, ical: &str, item: PlantedItem<'_>, sender: u32, created: Option<i64>) -> u32`
    - `pub async fn rewrite_inbox_item(test: &TestServer, account_id: u32, document_id: u32, edit: impl FnOnce(&mut CalendarEventNotification))`
  - `inbox::test(test: &mut TestServer)`, plus the `pub const KEY_INBOX`, `invitation` and `item_href` it shares with Task 7.

Background:
- Inbox items reach clients through two handlers:
  - The PROPFIND/REPORT loader in `handle_dav_query` (`propfind.rs:430-486`) serves PROPFIND and `sync-collection` on `/dav/itip/<user>/inbox/`. A sync is a `DavQueryResource::Uri` query (`crates/dav/src/common/mod.rs:253-278`).
  - GET and HEAD go to `handle_scheduling_get_request` (`scheduling.rs:69-150`).
- Two REPORTs never reach inbox items (from code reading):
  - `calendar-query` resolves its path against the calendar resources (`crates/dav/src/calendar/query.rs:70-85`), so it answers not-found for the inbox.
  - `calendar-multiget` is parented on `Calendar`, and the fork's per-href check answers 404 for an inbox href (`propfind.rs:1558-1562`).
- The `DavResources` cache knows no flags, so the skip happens where the archive is loaded: one call in the loader, one in GET.
  - A skipped item gets no response entry.
  - A PROPFIND whose only item is skipped returns 404 (`propfind.rs:1172-1176`).
- `za_item_hidden` returns `false` for non-key accounts without looking at the flag, so a plain account's inbox is upstream's whatever its flags hold (invariant 9).
- A key account's item without the bit goes through `za_item_view`:
  - a drained item is unsealed;
  - a plaintext one passes through;
  - a `pk`-wrapped one is an unseal failure: 500, or one 500 entry in a listing, and nothing changes (spec 10).
- DELETE (`scheduling.rs:152-236`) is upstream's and reads no tree, so it removes an item whatever its flags. Spec 6.6 hides pending items from listings, reports and GET only. The tests use DELETE for cleanup.

- [ ] **Step 1: Test helpers**

In `tests/src/utils/za.rs`, add imports:
- `calcard::icalendar::ICalendar`
- `common::storage::index::ObjectIndexBuilder`
- `groupware::calendar::{CalendarEventNotification, ChangedBy, EVENT_NOTIFICATION_ZA_PENDING}`
- `groupware::calendar::seal::{seal_drained_item, seal_pending_item}` (merge into the Task 5 seal import)
- `store::{ValueKey, write::{AlignedBytes, Archive}}`

Then append:

```rust
/// How `plant_inbox_item` stores an item (invitations spec 9).
pub enum PlantedItem<'a> {
    /// Sealed to the account's public key, read from its vault record (the
    /// fan-out's form, spec 5.3), pending bit set. `pair` is sealed into the
    /// root's `X-ZA-EXTRA` (a REQUEST or CANCEL); `reply_token` becomes the
    /// visible `X-ZA-RSVP-TOKEN` (a REPLY).
    Pending {
        pair: Option<(&'a str, [u8; 16])>,
        reply_token: Option<[u8; 16]>,
    },
    /// Sealed under the account's keys, unlocked from `password` as a login
    /// does (the drain's form, spec 6.4 step 5), pending bit clear.
    Drained { password: &'a str },
    /// Stored as given with `flags`: a plain account's item, or a key
    /// account's legacy plaintext one.
    Plain { flags: u16 },
}

fn parse_ical(ical: &str) -> ICalendar {
    match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    }
}

/// Writes `ical` into the account's scheduling inbox as a
/// `CalendarEventNotification` from `sender`, bypassing every handler: the
/// fan-out (plan 9) and the drain (plan 10) that will write such items do not
/// exist yet. `created` backdates the item (inbox expiry; upstream's
/// `insert` would overwrite it). Returns the document id.
pub async fn plant_inbox_item(
    test: &TestServer,
    account_id: u32,
    ical: &str,
    item: PlantedItem<'_>,
    sender: u32,
    created: Option<i64>,
) -> u32 {
    let mut tree = parse_ical(ical);
    let flags = match item {
        PlantedItem::Pending { pair, reply_token } => {
            let record = test
                .server
                .za_vault_record(account_id)
                .await
                .unwrap()
                .expect("vault record")
                .record;
            let public: [u8; 32] = record
                .public_key
                .as_slice()
                .try_into()
                .expect("public key");
            let pairs = pair.map(|(address, token)| {
                let mut pairs = TokenPairs::new();
                pairs.insert(address, token);
                pairs
            });
            seal_pending_item(
                &mut tree,
                account_id,
                &public,
                pairs.as_ref(),
                reply_token.as_ref(),
            )
            .unwrap();
            EVENT_NOTIFICATION_ZA_PENDING
        }
        PlantedItem::Drained { password } => {
            let keys = za_unlock(test, account_id, password).await;
            seal_drained_item(&mut tree, &keys, account_id).unwrap();
            0
        }
        PlantedItem::Plain { flags } => flags,
    };
    let now = store::write::now() as i64;
    let item = CalendarEventNotification {
        event: tree,
        event_id: None,
        changed_by: ChangedBy::PrincipalId(sender),
        flags,
        size: ical.len() as u32,
        created: created.unwrap_or(now),
        modified: now,
    };
    let account_info = test.server.account_info(account_id).await.unwrap();
    let document_id = test
        .server
        .store()
        .assign_document_ids(account_id, Collection::CalendarEventNotification, 1)
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::CalendarEventNotification)
        .with_document(document_id)
        .custom(
            ObjectIndexBuilder::<(), _>::new()
                .with_changes(item)
                .with_changed_by(account_info.account_tenant_ids()),
        )
        .unwrap()
        .commit_point();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::DavResources(account_id)])
        .await;
    document_id
}

/// Rewrites a planted item in place, same document id, as the drain's commit
/// will (spec 6.4 step 5): `edit` changes the stored item, and the write
/// asserts the archive it read.
pub async fn rewrite_inbox_item(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    edit: impl FnOnce(&mut CalendarEventNotification),
) {
    let stored = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .unwrap()
        .expect("inbox item");
    let mut item = stored
        .deserialize::<CalendarEventNotification>()
        .unwrap();
    edit(&mut item);
    item.modified = store::write::now() as i64;
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::CalendarEventNotification)
        .with_document(document_id)
        .custom(
            ObjectIndexBuilder::new()
                .with_current(
                    stored
                        .to_unarchived::<CalendarEventNotification>()
                        .unwrap(),
                )
                .with_changes(item)
                .with_changed_by(account_info.account_tenant_ids()),
        )
        .unwrap()
        .commit_point();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::DavResources(account_id)])
        .await;
}
```

- [ ] **Step 2: The inbox test**

Create `tests/src/za/inbox.rs` with the SPDX header and:

```rust
//! Invitations spec 6.6: the scheduling inbox as a key account's clients
//! see it. Items are planted straight into the store (`plant_inbox_item`):
//! the fan-out (plan 9) and the drain (plan 10) do not exist yet.

use super::STRONG;
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{PlantedItem, plant_inbox_item, rewrite_inbox_item},
};
use calcard::{Entry, Parser};
use dav_proto::Depth;
use groupware::calendar::EVENT_NOTIFICATION_ZA_PENDING;
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive},
};
use types::collection::Collection;

pub const KEY_INBOX: &str = "/dav/itip/key1%40example.com/inbox/";
const PLAIN_INBOX: &str = "/dav/itip/plain%40example.com/inbox/";
const PROPFIND_DATA: &str = "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><A:calendar-data/></D:prop></D:propfind>";

/// An invitation from key2 to key1, as the fan-out will deliver it.
pub fn invitation(uid: &str, canary: &str) -> String {
    format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nMETHOD:REQUEST\r\n",
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\n",
            "DTSTART:20990101T100000Z\r\nDTEND:20990101T110000Z\r\nSEQUENCE:0\r\n",
            "SUMMARY:{canary}\r\nORGANIZER:mailto:key2@example.com\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key1@example.com\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        uid = uid,
        canary = canary
    )
}

/// The href the resource cache gives an inbox item (`<document id>.ics`).
pub fn item_href(inbox: &str, document_id: u32) -> String {
    format!("{inbox}{document_id}.ics")
}

pub fn parse(text: &str) -> calcard::icalendar::ICalendar {
    match Parser::new(text).entry() {
        Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    }
}

pub async fn stored_item(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> Option<Archive<AlignedBytes>> {
    test.server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .unwrap()
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access inbox read path tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let sender = test.account("key2@example.com").id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);

    let pending_ical = invitation("za-inbox-pending", "inbox-pending-canary");
    let drained_ical = invitation("za-inbox-drained", "inbox-drained-canary");
    let legacy_ical = invitation("za-inbox-legacy", "inbox-legacy-canary");
    let pending = plant_inbox_item(
        test,
        id,
        &pending_ical,
        PlantedItem::Pending {
            pair: Some(("key1@example.com", [0x5A; 16])),
            reply_token: None,
        },
        sender,
        None,
    )
    .await;
    let drained = plant_inbox_item(
        test,
        id,
        &drained_ical,
        PlantedItem::Drained { password: STRONG },
        sender,
        None,
    )
    .await;
    // Review Focus 4: a plaintext item (no key envelope) passes through.
    let legacy = plant_inbox_item(
        test,
        id,
        &legacy_ical,
        PlantedItem::Plain { flags: 0 },
        sender,
        None,
    )
    .await;
    let pending_href = item_href(KEY_INBOX, pending);
    let drained_href = item_href(KEY_INBOX, drained);
    let legacy_href = item_href(KEY_INBOX, legacy);

    // Listing: the pending item is absent; the others show plaintext.
    let response = client
        .request_with_headers("PROPFIND", KEY_INBOX, [("depth", "1")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let hrefs: Vec<String> = response.hrefs().into_iter().map(str::to_string).collect();
    assert!(!hrefs.contains(&pending_href), "pending item listed: {hrefs:?}");
    assert!(
        hrefs.contains(&drained_href) && hrefs.contains(&legacy_href),
        "{hrefs:?}"
    );
    let body = response.body.unwrap();
    assert!(
        body.contains("inbox-drained-canary")
            && body.contains("inbox-legacy-canary")
            && !body.contains("inbox-pending-canary")
            && !body.contains("X-ZA-")
            && !body.contains("HTTP/1.1 500"),
        "{body}"
    );

    // GET, and Review Focus 2: HEAD and PROPFIND depth 0, all 404.
    client
        .request("GET", &pending_href, "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    client
        .request("HEAD", &pending_href, "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    client
        .request_with_headers("PROPFIND", &pending_href, [("depth", "0")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::NOT_FOUND);
    // Review Focus 1: a multiget naming it answers 404 for that href.
    let body = client
        .multiget_calendar(KEY_INBOX, &[pending_href.as_str()])
        .await
        .response
        .body
        .unwrap();
    assert!(
        body.contains(&pending_href)
            && body.contains("HTTP/1.1 404")
            && !body.contains("inbox-pending-canary"),
        "{body}"
    );
    // An initial sync-collection does not report it either.
    let body = client
        .sync_collection(KEY_INBOX, "", Depth::One, None, ["D:getetag"])
        .await
        .body
        .unwrap();
    assert!(
        !body.contains(&pending_href) && body.contains(&drained_href),
        "{body}"
    );

    // GET of the drained and the legacy item: their plaintext trees.
    for (href, ical) in [(&drained_href, &drained_ical), (&legacy_href, &legacy_ical)] {
        let body = client
            .request("GET", href, "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap();
        assert!(!body.contains("X-ZA-"), "{body}");
        assert_eq!(parse(&body), parse(ical), "{href}");
    }
    let stored = stored_item(test, id, drained).await.unwrap();
    assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("inbox-drained-canary"));

    // Review Focus 3: a `pk`-wrapped item whose pending bit was cleared is an
    // unseal failure: GET 500, one 500 entry in the listing, nothing changed.
    rewrite_inbox_item(test, id, pending, |item| {
        item.flags &= !EVENT_NOTIFICATION_ZA_PENDING
    })
    .await;
    let before = stored_item(test, id, pending).await.unwrap();
    client
        .request("GET", &pending_href, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let body = client
        .request_with_headers("PROPFIND", KEY_INBOX, [("depth", "1")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(
        body.contains(&pending_href)
            && body.contains("HTTP/1.1 500")
            && body.contains("inbox-drained-canary")
            && !body.contains("inbox-pending-canary"),
        "{body}"
    );
    let after = stored_item(test, id, pending).await.expect("nothing deleted");
    assert_eq!(after.as_bytes(), before.as_bytes(), "nothing rewritten");

    // Control: a plain account's inbox is upstream's; the bit means nothing.
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_client =
        DummyWebDavClient::new(plain_id, plain.name(), plain.secret(), plain.name());
    let plain_ical = invitation("za-inbox-plain", "inbox-plain-canary");
    let plain_item = plant_inbox_item(
        test,
        plain_id,
        &plain_ical,
        PlantedItem::Plain {
            flags: EVENT_NOTIFICATION_ZA_PENDING,
        },
        sender,
        None,
    )
    .await;
    let plain_href = item_href(PLAIN_INBOX, plain_item);
    let hrefs: Vec<String> = plain_client
        .request_with_headers("PROPFIND", PLAIN_INBOX, [("depth", "1")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .hrefs()
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(hrefs.contains(&plain_href), "{hrefs:?}");
    let body = plain_client
        .request("GET", &plain_href, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(parse(&body), parse(&plain_ical));

    // Cleanup: upstream's DELETE removes an item whatever its flags.
    for href in [&pending_href, &drained_href, &legacy_href] {
        client
            .request("DELETE", href, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    plain_client
        .request("DELETE", &plain_href, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}
```

In `tests/src/za/mod.rs`, add `pub mod inbox;` after `pub mod gating;`, and `inbox::test(&mut test).await;` right after `pairs::test(&mut test).await;`.

- [ ] **Step 3: Run to see it fail**

Run the za suite.
Expected: FAIL at `pending item listed`. Today the loader borrows every inbox archive unchanged (`za_archive_view`'s `_ =>` arm), so the pending item is listed with its sealed tree.

- [ ] **Step 4: Implement the views and the skip**

`crates/dav/src/common/za.rs`. Replace the groupware import (`:9`) with:

```rust
use groupware::calendar::{
    CalendarEventNotification,
    seal::{
        archived_item_is_pending, seal_error, unseal_calendar_archive, unseal_event_archive,
        unseal_item_archive,
    },
};
```

After `za_calendar_view` (`:142-155`):

```rust
/// Invitations spec 6.6: the inbox counterpart of [`za_event_view`], same
/// contract. A drained item is unsealed under the account's keys; a pending
/// (`pk`) item reached here is an unseal failure (spec 10): the caller
/// answers 500 for it and changes nothing.
pub(crate) fn za_item_view<'x>(
    stored: &'x Archive<AlignedBytes>,
    keys: Option<&Arc<SessionKeys>>,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Cow<'x, Archive<AlignedBytes>>> {
    match keys {
        Some(keys) => unseal_item_archive(stored, keys, account_id)
            .map(Cow::Owned)
            .map_err(|err| seal_error(err, account_id, document_id)),
        None => Ok(Cow::Borrowed(stored)),
    }
}

/// Invitations spec 6.6: whether a loaded archive is a key account's pending
/// inbox item, which clients do not see until the drain rewrites it. `false`
/// for every other collection and for non-key accounts (invariant 9). The
/// resource cache knows no flags, so readers ask here after loading.
pub(crate) async fn za_item_hidden(
    server: &Server,
    account_id: u32,
    collection: Collection,
    stored: &Archive<AlignedBytes>,
) -> crate::Result<bool> {
    if collection != Collection::CalendarEventNotification
        || !is_key_account(server, account_id).await?
    {
        return Ok(false);
    }
    Ok(archived_item_is_pending(
        stored
            .unarchive::<CalendarEventNotification>()
            .caused_by(trc::location!())?,
    ))
}
```

In `za_archive_view` (`:169-179`), add an arm before `_ =>`. Update its doc comment: "only calendar collections, events and inbox items consult the gate".

```rust
        Collection::CalendarEventNotification => {
            let keys = server.za_session_keys(access_token, account_id).await?;
            za_item_view(stored, keys.as_ref(), account_id, document_id)
        }
```

`crates/dav/src/common/propfind.rs`:
- In the import (`:24`), replace `za::za_archive_view` with `za::{za_archive_view, za_item_hidden}`.
- Right after `stored_ = archive;` (`:458`), insert:

```rust
                // Invitations spec 6.6: a pending inbox item is not listed.
                if za_item_hidden(self, account_id, collection, &stored_).await? {
                    continue;
                }
```

`crates/dav/src/calendar/scheduling.rs`:
- In the `common::{..}` import (`:10-14`), add `za::{ZeroAccessGate, za_item_hidden, za_item_view},`.
- Replace `:116-118` (`let event = event_.unarchive::<CalendarEventNotification>()...`) with:

```rust
        // Invitations spec 6.6: a pending item answers 404 until drained; a
        // drained one is shown unsealed. Identity stays the stored archive's.
        if za_item_hidden(
            self,
            account_id,
            Collection::CalendarEventNotification,
            &event_,
        )
        .await?
        {
            return Err(DavError::Code(StatusCode::NOT_FOUND));
        }
        let za_keys = self.za_session_keys(access_token, account_id).await?;
        let view_ = za_item_view(&event_, za_keys.as_ref(), account_id, resource.document_id())?;
        let event = view_
            .unarchive::<CalendarEventNotification>()
            .caused_by(trc::location!())?;
```

`let etag = event_.etag();` (`:121`) stays bound to the stored archive.

- [ ] **Step 5: Run green**

Run the za suite. Expected: PASS.

Fallbacks:
- The PROPFIND depth 0 check answers 207 instead of 404 (from code reading, `propfind.rs:1172-1176`). Assert that the response holds no response entry for the pending href, and record a ruling.
- The multiget answers a status line other than `HTTP/1.1 404` for the inbox href. Stop and report: the per-href collection check behaves differently from the code reading.

- [ ] **Step 6: Red runs (M16, M17, M21)**

One at a time. Before the next run, restore the deleted code exactly as Step 4 shows it and confirm with `git diff -- <file>` that only Step 4's change remains. Write each failure line into the task report.
1. **M16, loader skip.** In `crates/dav/src/common/propfind.rs`, delete the three-line `za_item_hidden` block. Run the za suite. Expected: FAIL at `pending item listed`. The listing now carries the item with a 500 entry.
2. **M17, GET 404.** In `crates/dav/src/calendar/scheduling.rs`, delete the `if za_item_hidden(..) { return Err(..NOT_FOUND) }` block. Run the za suite. Expected: FAIL at the pending GET, `Expected 404 Not Found but got 500 Internal Server Error`.
3. **M21, the view arm.** In `crates/dav/src/common/za.rs`, delete the `Collection::CalendarEventNotification` arm of `za_archive_view`. Run the za suite. Expected: FAIL at the listing body assertion: the drained item's sealed tree, with `X-ZA-` carriers and no canary.

- [ ] **Step 7: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p dav -- --check
cargo fmt -p tests -- --check
git add crates/dav/src/common/za.rs crates/dav/src/common/propfind.rs crates/dav/src/calendar/scheduling.rs tests/src/utils/za.rs tests/src/za/inbox.rs tests/src/za/mod.rs
git commit -m "Inbox: pending items hidden, drained items unsealed"
```

---

### Task 7: `sync-collection` ruling and inbox expiry

**Files:**
- Modify: `tests/src/za/inbox.rs` (new `test_sync`, `test_expiry`)
- Modify: `tests/src/za/mod.rs` (run both after `inbox::test`)

**Interfaces:**
- Consumes: Task 6's `plant_inbox_item`, `rewrite_inbox_item`, `KEY_INBOX`, `invitation`, `item_href`, `parse`, `stored_item`; Task 5's `za_unlock`; Task 3's `seal_drained_item`.
- Produces: `inbox::test_sync`, `inbox::test_expiry`, and the `sync-collection` ruling for the outcome note (Task 9).

Background:
- Spec 13 leaves open how a drained item reports in `sync-collection`. On the inbox, `get()` (`crates/dav/src/common/propfind.rs:1315-1337`) merges `InsertItem` and `UpdateItem` changes into one bitmap, and the loader then skips or shows each item.
- A rewrite in place logs `UpdateItem` (`ObjectIndexBuilder` with a current archive, `crates/common/src/storage/index.rs:347-366`).
- So a sync reports the drained item once from any token taken before the rewrite: a member with a 200 propstat, which on the wire is indistinguishable from a new member (RFC 6578) (from code reading).
- If a run shows the item missing, or reported twice, stop and report: a drained invitation that never syncs is a client-visible bug, not a ruling to absorb.
- Expiry: upstream's `itip_auto_expunge` (`crates/groupware/src/calendar/storage.rs:84-168`) deletes items by the creation time in their `CreatedToId` index and opens no tree.
  - `purge_account` calls it with the hold period when `expungeSchedulingInboxAfter` is set (`crates/email/src/message/delete.rs:150-164`).
  - The default is 30 days (`crates/registry/src/schema/structs_impl.rs:6604`), and the za suite does not change `DataRetention` (from code reading).
- The harness reaches that path without a product change: an administrator creates a `Task::AccountMaintenance` with type `Purge` through the registry (as `tests/src/system/archiving.rs:118-125` does for `StoreMaintenance`), and `wait_for_tasks` waits for it. The task runs in the task manager with no session of the account, which is the point.

- [ ] **Step 1: Write both tests**

Add to `tests/src/za/inbox.rs`'s imports:
- `crate::utils::za::za_unlock`
- `groupware::calendar::seal::seal_drained_item`
- `::registry::schema::{enums::TaskAccountMaintenanceType, structs::{Task, TaskAccountMaintenance, TaskStatus}}`. Use the `::` prefix, because `tests::za` has a module named `registry`.
- `store::write::now`

Append:

```rust
/// Spec 13: a pending item drained between two syncs. The resource cache
/// knows no flags, so the item is skipped where it is loaded; once rewritten
/// in its drained form (same document id, bit cleared) a sync reports it.
pub async fn test_sync(test: &mut TestServer) {
    println!("Running zero-access inbox sync-collection test...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let sender = test.account("key2@example.com").id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let keys = za_unlock(test, id, STRONG).await;

    let t0 = client
        .sync_collection(KEY_INBOX, "", Depth::One, None, ["D:getetag"])
        .await
        .sync_token()
        .to_string();
    let ical = invitation("za-inbox-sync", "inbox-sync-canary");
    let item = plant_inbox_item(
        test,
        id,
        &ical,
        PlantedItem::Pending {
            pair: Some(("key1@example.com", [0x33; 16])),
            reply_token: None,
        },
        sender,
        None,
    )
    .await;
    let href = item_href(KEY_INBOX, item);
    let reported = |hrefs: Vec<&str>| hrefs.iter().filter(|h| **h == href).count();

    let response = client
        .sync_collection(KEY_INBOX, &t0, Depth::One, None, ["D:getetag"])
        .await;
    assert_eq!(reported(response.hrefs()), 0, "a pending item was reported");
    let t1 = response.sync_token().to_string();

    rewrite_inbox_item(test, id, item, |stored| {
        stored.event = parse(&ical);
        seal_drained_item(&mut stored.event, &keys, id).unwrap();
        stored.flags &= !EVENT_NOTIFICATION_ZA_PENDING;
    })
    .await;

    for (token, taken) in [(&t1, "while it was pending"), (&t0, "before it arrived")] {
        let response = client
            .sync_collection(KEY_INBOX, token, Depth::One, None, ["D:getetag"])
            .await;
        assert_eq!(
            reported(response.hrefs()),
            1,
            "drained item from a token taken {taken}: {:?}",
            response.hrefs()
        );
        let body = response.body.as_deref().unwrap_or_default();
        assert!(body.contains("HTTP/1.1 200"), "{body}");
    }
    client
        .request("GET", &href, "")
        .await
        .with_status(StatusCode::OK);
    client
        .request("DELETE", &href, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}

/// Spec 6.6: upstream's inbox expiry deletes by creation time and needs no
/// key, so it also removes pending items older than the hold period.
pub async fn test_expiry(test: &mut TestServer) {
    println!("Running zero-access inbox expiry test...");
    let admin = test.account("admin@example.com").clone();
    let key1 = test.account("key1@example.com").clone();
    let id = key1.id().document_id();
    let sender = test.account("key2@example.com").id().document_id();
    let client = DummyWebDavClient::new(id, key1.name(), STRONG, key1.name());
    let pending = || PlantedItem::Pending {
        pair: None,
        reply_token: None,
    };
    // Older than the default 30-day `expungeSchedulingInboxAfter`.
    let old = plant_inbox_item(
        test,
        id,
        &invitation("za-inbox-old", "inbox-old-canary"),
        pending(),
        sender,
        Some(now() as i64 - 31 * 86_400),
    )
    .await;
    let fresh = plant_inbox_item(
        test,
        id,
        &invitation("za-inbox-fresh", "inbox-fresh-canary"),
        pending(),
        sender,
        None,
    )
    .await;

    // The account purge, as the scheduler runs it: no session of key1.
    admin
        .registry_create_object(Task::AccountMaintenance(TaskAccountMaintenance {
            account_id: key1.id(),
            maintenance_type: TaskAccountMaintenanceType::Purge,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
    assert!(
        stored_item(test, id, old).await.is_none(),
        "an expired pending item survived the purge"
    );
    assert!(
        stored_item(test, id, fresh).await.is_some(),
        "the purge removed a fresh item"
    );

    client
        .request("DELETE", &item_href(KEY_INBOX, fresh), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}
```

In `tests/src/za/mod.rs`, after `inbox::test(&mut test).await;`, add:

```rust
    inbox::test_sync(&mut test).await;
    inbox::test_expiry(&mut test).await;
```

- [ ] **Step 2: Run**

Run the za suite. Expected: PASS (from code reading).

Fallbacks:
- **`registry_create_object` refuses `Task::AccountMaintenance`.** Create `Task::StoreMaintenance(TaskStoreMaintenance { maintenance_type: TaskStoreMaintenanceType::PurgeAccounts, shard_index: None, status: TaskStatus::now() })` instead, as `tests/src/system/archiving.rs:118-125` does. That purges every account. Record the change as a ruling.
- **The old item survives with the default hold period.** Set `DataRetention { expunge_scheduling_inbox_after: Some(3_600_000u64.into()), .. }` with `Property::ExpungeSchedulingInboxAfter` through `admin.registry_update_setting`, then `admin.reload_settings()`, before creating the task, as `archiving.rs:33-46` sets its retention. Plant `old` at `now - 7_200`. Record a ruling naming the default the run observed.
- **The sync assertions fail.** Stop and report with the response bodies (see Background).

- [ ] **Step 3: Record the ruling**

Write the `sync-collection` behaviour the run showed into the task report, for the outcome note. State whether the item was reported from both tokens, how many times, and with which propstat status.

- [ ] **Step 4: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git add tests/src/za/inbox.rs tests/src/za/mod.rs
git commit -m "Inbox: sync-collection after a drain, and expiry of pending items"
```

---

### Task 8: Leak coverage for inbox items and tokens

**Files:**
- Modify: `tests/src/za/leak.rs`:
  - imports (`:10-44`), `EXTRA_CANARIES` (`:51-68`), constants after `SCHED_EVENT` (`:72-79`)
  - `Scan` (`:120-131`), new `check_item` after `check_event` (`:220-232`)
  - `scan`'s property branch (`:383-401`), new `token_hits` and `archive_key` after `scan` (`:298-453`)
  - `test` (`:455-789`)

**Interfaces:**
- Consumes: Task 6's `plant_inbox_item`, `PlantedItem`; Task 5's `plant_sealed_event`; Task 3's `RSVP_TOKEN_PROP`, `EVENT_NOTIFICATION_ZA_PENDING`; Task 2's `TokenPairs`.
- Produces:
  - `Scan.items`, `Scan.item_keys`
  - `pub async fn token_hits(test: &TestServer, tokens: &[[u8; 16]]) -> Vec<(u8, Vec<u8>, usize)>`
  - `fn archive_key(account_id: u32, collection: Collection, document_id: u32) -> Vec<u8>`
  - Plan 10's flow leak tests reuse `token_hits` and the item check.

Background:
- Spec 11, leak tests:
  - Inbox items join the whole-store scan.
  - The sealed-tree check covers them, with an `X-ZA-KEY` count greater than zero.
  - A token-bytes check finds no token in the clear except on a pending reply item.
- The scan already walks every subspace, the scheduling inbox's archives included, for canaries. What is new is the structural check of inbox item archives and the token search.
- A token "in the clear" means any of these forms appears in a raw key, a raw value, a decoded archive or a decoded blob:
  - its 16 raw bytes;
  - base64url without padding, the `X-ZA-RSVP-TOKEN` form;
  - standard base64.
- The tokens are ASCII strings so a hit is readable in the report.
- Planted here, in key2's inbox and `leak` calendar:
  - a pending REQUEST, whose pair is in the sealed `X-ZA-EXTRA`;
  - a pending REPLY, whose token is visible;
  - a drained item;
  - an event with a format 2 bundle of two pairs.
- The event is deleted with the `leak` calendar at the end of the module; the items are deleted through DAV.
- A negative control (a plaintext item with the pending bit) proves the item check is not blind. The visible reply token is the positive control for the token search.

- [ ] **Step 1: Extend the scanner**

Imports: add `crate::utils::za::{PlantedItem, plant_inbox_item, plant_sealed_event}`, `groupware::calendar::{CalendarEventNotification, EVENT_NOTIFICATION_ZA_PENDING}`, `groupware::calendar::seal::{TokenPairs, item::RSVP_TOKEN_PROP}`, and `base64::{Engine, engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD}}`.

Append to `EXTRA_CANARIES`:

```rust
    "itip-request-canary",
    "itip-reply-canary",
    "itip-drained-canary",
    "itip-pair-canary",
    "itip-negative-canary",
```

After `SCHED_EVENT`:

```rust
/// Inbox items planted for the scan (invitations spec 11): a pending REQUEST
/// to key2 with key2's pair, key2's pending REPLY from plain, a drained item.
const ITIP_REQUEST: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\n",
    "UID:leak-itip-request\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
    "DTEND:20990102T100000Z\r\nSEQUENCE:0\r\nSUMMARY:itip-request-canary\r\n",
    "ORGANIZER:mailto:plain@example.com\r\n",
    "ATTENDEE;CN=itip-request-canary:mailto:key2@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
const ITIP_REPLY: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\n",
    "UID:leak-sched\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
    "SEQUENCE:0\r\nORGANIZER:mailto:key2@example.com\r\n",
    "ATTENDEE;PARTSTAT=ACCEPTED:mailto:plain@example.com\r\n",
    "COMMENT:itip-reply-canary\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
const ITIP_DRAINED: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\n",
    "UID:leak-itip-drained\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990103T090000Z\r\n",
    "DTEND:20990103T100000Z\r\nSEQUENCE:0\r\nSUMMARY:itip-drained-canary\r\n",
    "ORGANIZER:mailto:plain@example.com\r\n",
    "ATTENDEE:mailto:key2@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
/// Tokens planted with them (16 ASCII bytes each).
const TOKEN_REQUEST: [u8; 16] = *b"za-tok-request-1";
const TOKEN_REPLY: [u8; 16] = *b"za-tok-reply-002";
const TOKEN_EVENT_A: [u8; 16] = *b"za-tok-event-a03";
const TOKEN_EVENT_B: [u8; 16] = *b"za-tok-event-b04";
```

In `Scan`, add after `calendars`:

```rust
    pub items: usize,
    pub item_keys: usize,
```

After `check_event`:

```rust
/// An inbox item (invitations spec 9): the root ends with `X-ZA-KEY` and,
/// before it, at most one of `X-ZA-RSVP-TOKEN` (visible, a pending reply)
/// or `X-ZA-EXTRA`; the rest as `check_tree` checks any component. Returns
/// whether the item carries its key envelope.
fn check_item(item: &CalendarEventNotification, what: &str, violations: &mut Vec<String>) -> bool {
    let mut tree = item.event.clone();
    let has_key = tree
        .components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| carrier_kind(e) == Some(KEY_PROP));
    if !has_key {
        violations.push(format!("{what}: missing {KEY_PROP}"));
    } else {
        let root = &mut tree.components[0].entries;
        root.pop();
        if root.last().is_some_and(|e| {
            carrier_kind(e) == Some(EXTRA_PROP)
                || matches!(&e.name, ICalendarProperty::Other(n) if n.eq_ignore_ascii_case(RSVP_TOKEN_PROP))
        }) {
            root.pop();
        }
    }
    check_tree(&tree, what, false, violations);
    has_key
}
```

In `scan`'s property branch, after the `Collection::Calendar` arm (`:391-401`), add:

```rust
            } else if collection == Collection::CalendarEventNotification as u8 {
                scan.items += 1;
                match archive.deserialize::<CalendarEventNotification>() {
                    Ok(item) => {
                        if check_item(&item, &what, &mut scan.violations) {
                            scan.item_keys += 1;
                        }
                    }
                    Err(err) => scan
                        .violations
                        .push(format!("{what}: inbox item archive does not decode: {err:?}")),
                }
```

After `scan`:

```rust
/// The property-subspace key of an archive: account id (u32 BE) |
/// collection | `Field::ARCHIVE` | document id (u32 BE).
fn archive_key(account_id: u32, collection: Collection, document_id: u32) -> Vec<u8> {
    let mut key = account_id.to_be_bytes().to_vec();
    key.push(collection as u8);
    key.push(u8::from(Field::ARCHIVE));
    key.extend_from_slice(&document_id.to_be_bytes());
    key
}

/// Where any of `tokens` appears in the clear (invitations spec 11): as raw
/// bytes, base64url without padding (the `X-ZA-RSVP-TOKEN` form) or
/// standard base64, in a raw key, a raw value, a decoded archive or a
/// decoded blob. Returns (subspace, key, token index) per hit; a blob hit
/// carries `SUBSPACE_BLOBS` and the blob hash.
pub async fn token_hits(test: &TestServer, tokens: &[[u8; 16]]) -> Vec<(u8, Vec<u8>, usize)> {
    let forms: Vec<(usize, Vec<u8>)> = tokens
        .iter()
        .enumerate()
        .flat_map(|(i, token)| {
            [
                (i, token.to_vec()),
                (i, URL_SAFE_NO_PAD.encode(token).into_bytes()),
                (i, STANDARD.encode(token).into_bytes()),
            ]
        })
        .collect();
    let find = |bytes: &[u8]| -> Vec<usize> {
        forms
            .iter()
            .filter(|(_, form)| bytes.windows(form.len()).any(|w| w == form.as_slice()))
            .map(|(i, _)| *i)
            .collect()
    };
    let store = test.server.store().clone();
    let mut hits = Vec::new();
    let mut blob_hashes: Vec<Vec<u8>> = Vec::new();
    for &subspace in SUBSPACES {
        if subspace == SUBSPACE_SEARCH_INDEX && store.is_pg_or_mysql() {
            continue;
        }
        let mut records: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        store
            .iterate(
                IterateParams::new(
                    AnyKey {
                        subspace,
                        key: vec![0u8],
                    },
                    AnyKey {
                        subspace,
                        key: vec![u8::MAX; 64],
                    },
                ),
                |key, value| {
                    records.push((key.to_vec(), value.to_vec()));
                    Ok(true)
                },
            )
            .await
            .unwrap();
        for (key, value) in records {
            let mut found = find(&key);
            found.extend(find(&value));
            if let Some(archive) = try_archive(&value) {
                found.extend(find(archive.as_bytes()));
            }
            if subspace == SUBSPACE_BLOBS {
                blob_hashes.push(key.clone());
            } else if subspace == SUBSPACE_BLOB_LINK && key.len() >= BLOB_HASH_LEN {
                blob_hashes.push(key[..BLOB_HASH_LEN].to_vec());
            }
            found.sort_unstable();
            found.dedup();
            hits.extend(found.into_iter().map(|i| (subspace, key.clone(), i)));
        }
    }
    blob_hashes.sort_unstable();
    blob_hashes.dedup();
    for hash in blob_hashes {
        if let Ok(Some(blob)) = test
            .server
            .blob_store()
            .get_blob(&hash, 0..usize::MAX)
            .await
        {
            hits.extend(find(&blob).into_iter().map(|i| (SUBSPACE_BLOBS, hash.clone(), i)));
        }
    }
    hits
}
```

Add `{} items` to the scan summary `println!` (`:663-673`), with `result.items` as its argument.

- [ ] **Step 2: Plant, assert, control and clean up in `leak::test`**

Right before `let result = scan(test, id).await;` (`:662`), insert:

```rust
    // Invitations spec 11: inbox items in both states and an event whose
    // bundle holds token pairs, as plans 9 and 10 will write them.
    let sender = test.account("plain@example.com").id().document_id();
    let request_id = plant_inbox_item(
        test,
        id,
        ITIP_REQUEST,
        PlantedItem::Pending {
            pair: Some(("itip-pair-canary@example.com", TOKEN_REQUEST)),
            reply_token: None,
        },
        sender,
        None,
    )
    .await;
    let reply_id = plant_inbox_item(
        test,
        id,
        ITIP_REPLY,
        PlantedItem::Pending {
            pair: None,
            reply_token: Some(TOKEN_REPLY),
        },
        sender,
        None,
    )
    .await;
    let drained_id = plant_inbox_item(
        test,
        id,
        ITIP_DRAINED,
        PlantedItem::Drained { password: STRONG },
        sender,
        None,
    )
    .await;
    let mut pairs = TokenPairs::new();
    pairs.insert("itip-pair-canary@example.com", TOKEN_EVENT_A);
    pairs.insert("key2@example.com", TOKEN_EVENT_B);
    plant_sealed_event(
        test,
        id,
        "leak",
        "pairs.ics",
        &EVENT.replace("za-event-1", "za-leak-pairs"),
        STRONG,
        &pairs,
    )
    .await;
```

After the existing counter assertions (`:684-691`), insert:

```rust
    // Inbox items were decoded and each carries its key envelope.
    assert!(result.items >= 3, "{result:?}");
    assert_eq!(result.item_keys, result.items, "{result:?}");

    // Tokens are in the clear only on the pending reply item, whose
    // `X-ZA-RSVP-TOKEN` is visible by design (spec 3.3); that hit is the
    // positive control.
    let hits = token_hits(
        test,
        &[TOKEN_REQUEST, TOKEN_REPLY, TOKEN_EVENT_A, TOKEN_EVENT_B],
    )
    .await;
    let reply_key = archive_key(id, Collection::CalendarEventNotification, reply_id);
    let on_reply =
        |subspace: u8, key: &[u8], token: usize| subspace == SUBSPACE_PROPERTY && key == reply_key.as_slice() && token == 1;
    assert!(
        hits.iter().any(|(s, k, t)| on_reply(*s, k, *t)),
        "the visible reply token was not found: {hits:?}"
    );
    let stray: Vec<_> = hits
        .iter()
        .filter(|(s, k, t)| !on_reply(*s, k, *t))
        .collect();
    assert!(
        stray.is_empty(),
        "a token in the clear outside the pending reply item: {stray:?}"
    );
```

After the existing negative-control block (it ends at `:779`), insert:

```rust
    // Negative control for items: a plaintext item with the pending bit is
    // caught by the canary search and by the item check.
    {
        let negative = plant_inbox_item(
            test,
            id,
            &ITIP_DRAINED.replace("itip-drained-canary", "itip-negative-canary"),
            PlantedItem::Plain {
                flags: EVENT_NOTIFICATION_ZA_PENDING,
            },
            sender,
            None,
        )
        .await;
        let key = format!(
            "{:?}",
            archive_key(id, Collection::CalendarEventNotification, negative)
        );
        let planted = scan(test, id).await;
        let report = planted.violations.join("\n");
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains("itip-negative-canary")),
            "the scanner must catch the planted item canary:\n{report}"
        );
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains(&key) && v.ends_with(&format!("missing {KEY_PROP}"))),
            "the scanner must catch the unsealed item:\n{report}"
        );
        client
            .request(
                "DELETE",
                &format!("/dav/itip/key2%40example.com/inbox/{negative}.ics"),
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for item in [request_id, reply_id, drained_id] {
        client
            .request(
                "DELETE",
                &format!("/dav/itip/key2%40example.com/inbox/{item}.ics"),
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
```

- [ ] **Step 3: Run green, plain and filesystem blob store**

Run the za suite, then the same with `BLOB_STORE=FileSystem` (CI's second za step). Expected: both PASS. The summary line shows `3 items` or more.

Fallbacks:
- **The positive control finds no hit.** The reply item's archive may be stored LZ4-compressed under a key other than `archive_key`. Print the hits for `TOKEN_REPLY` alone, confirm they lie on the reply item's record, and adjust `archive_key`. Record a ruling.
- **A stray hit names another record.** That is a leak or a planted-data mistake. Stop and report with the hit.

- [ ] **Step 4: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git add tests/src/za/leak.rs
git commit -m "Leak scan: inbox items in both states, tokens in the clear only on pending replies"
```

---

### Task 9: Mutation runs, final verification and the outcome note

**Files:**
- Modify (temporarily, reverted after each run): the product files in the table below
- Create: `docs/superpowers/plans/2026-10-13-zero-access-plan8-outcome.md`

**Interfaces:**
- Consumes: every test from Tasks 1-8.
- Produces: the outcome note, which plans 9 and 10 read before starting.

How a run works:
1. Apply exactly the edit in the table.
2. Run the named command.
3. Record the first failure as `file:line` plus the panic message, or "passed".
4. Revert with `git checkout -- <file>`. Confirm `git status --short crates/` is empty before the next mutation.

| Id | What it removes | Edit | Command | Expected first failure (from code reading) |
|----|-----------------|------|---------|-------------------------------------------|
| M16 | Pending skip in the loader | `crates/dav/src/common/propfind.rs`: delete the `za_item_hidden` block after `stored_ = archive;` | za suite | `inbox.rs`, `pending item listed` |
| M17 | Pending 404 on GET | `crates/dav/src/calendar/scheduling.rs`: delete the `if za_item_hidden(..) { .. NOT_FOUND .. }` block | za suite | `inbox.rs`, pending GET: `Expected 404 Not Found but got 500 Internal Server Error` |
| M18 | Pairs at the update seal site | `crates/dav/src/calendar/update.rs`: `seal_event(&mut new_event, keys, account_id, None)` in the update branch | za suite | `pairs.rs`, `no token pairs after an alarm-only PUT` |
| M19 | Pairs at the PROPPATCH seal site | `crates/dav/src/calendar/proppatch.rs`: `None` instead of `pairs.as_ref()` | za suite | `pairs.rs`, `no token pairs after a PROPPATCH` |
| M20 | Carrier order on pending items | `crates/groupware/src/calendar/seal/item.rs`, `open_pending_item`: delete the `has_stray_carriers` check | `cargo test -p groupware seal::item` | `pending_item_carriers_out_of_order_are_stray`, case `swapped` |
| M21 | The inbox view arm | `crates/dav/src/common/za.rs`: delete the `Collection::CalendarEventNotification` arm of `za_archive_view` | za suite | `inbox.rs`, the listing body assertion (sealed tree, no canary) |

- [ ] **Step 1: Run M16-M21**

Run them in order. Keep raw notes in the task report: the edit, the first failure, the revert confirmation.

If a mutation passes, decide which case applies:
- **(a) A request can still reach the removed check.** Write the missing assertion in the module that owns the check (under about forty lines), red-run it, and update the row.
- **(b) An earlier check stops every input.** Name that check and its line, and mark the row "layered behind <check>".

- [ ] **Step 2: Final verification**

Run in this order, never two at once:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test -p vault -p groupware
cargo test -p common auth::vault
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture
BLOB_STORE=FileSystem STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture
ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture
cargo build --release -p stalwart --no-default-features --features rocks
git status --short crates/   # must print nothing
```

Expected: all PASS. The release build adds no warning in `vault`, `groupware`, `common/src/auth` or `dav`.

Upstream's `cal_itip` sub-test of `webdav_tests` is timing-flaky; rerun once before treating a failure there as a regression. The key-mode `cal_scheduling` variant still asserts an empty inbox for jane, since plan 8 delivers nothing.

- [ ] **Step 3: Write the outcome note**

Create `docs/superpowers/plans/2026-10-13-zero-access-plan8-outcome.md` with exactly these sections. Fill each one from the task reports.

````markdown
# Zero-access calendar: plan 8 outcome

Written <date> at the end of plan 8 (`2026-10-13-zero-access-8-sealed-items.md`).
Read it before plans 9 and 10.

## Where things stand

- Commits `<first>..<last>` on `<branch>`; the suites run in Task 9 Step 2 and their results.
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

<copy the table "Names this plan settles" from the plan, with any change a task made>

## Rulings

- `sync-collection` after a drain: <Task 7 Step 3: reported from which tokens, how often, with which status>.
- Inbox expiry: reached through `Task::AccountMaintenance` (type `Purge`); default hold <observed>; <fallback taken, if any>.
- `assert_is_empty`: za sub-modules do not call it; they clean up, and the suite end checks (plan Global Constraints).
- `hkdf_expand` takes the full HKDF info, so the `pk` derivation appends `E || P` to the label like `derive_app_kek`; no sibling function.
- Format 2 bundles bind their format byte into the associated data.
- Pending key envelopes and item bundles bind the recipient and the UID (`za/v1|pk-dek|<r>|<uid>`, `za/v1|itip-extra|<r>|<uid>`), as release 1 events' do; the spec's section 9 table was amended with this plan.
- Any other ruling from the task reports.

## Mutation runs (plan 8)

| Id | Check, `file:line` | Edit | First failure | Command |
|----|--------------------|------|---------------|---------|
<one row per M16-M21, from Task 9 Step 1>

## Facts for plans 9 and 10

- Plan 9 calls:
  - `seal_pending_item(ical, recipient_id, &public_key, pair, reply_token)`: one pair or one token, never both; a low-order public key is `Structure("recipient public key")`.
  - `TokenPairs::{insert, get, only, matches}`.
  - `event_token_pairs` and `seal_event(.., Some(&pairs))`.
  - `EVENT_NOTIFICATION_ZA_PENDING`.
  - The create branch's `None` in `crates/dav/src/calendar/update.rs` becomes the organizer's pairs.
- Plan 10 calls:
  - `Server::za_private_key(&keys)`, once per pass; its error is already traced for the session.
  - `open_pending_item` (returns `PendingCarriers`), `seal_drained_item`, `archived_item_is_pending`, `TokenPairs::matches`.
  - `rewrite_inbox_item` and `plant_inbox_item` (`tests/src/utils/za.rs`) for its tests.
- DELETE of a pending item by its href is upstream's DELETE, unchanged; spec 6.6 hides items from listings, reports and GET only.
- A pending item's envelope and bundle bind the UID, so an item whose sealed parts were moved onto another UID's record does not open; the drain treats that as a permanent open failure (spec 6.5). Component bundles bind the account, the `itip` scope and the component index.

## Deferred findings

<findings from task and branch reviews that were not fixed, each with its reason>
````

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-10-13-zero-access-plan8-outcome.md
git commit -m "Record plan 8 mutation runs and rulings"
```
