# Plan 17, group C: tokens that carry keys (Tasks 13 to 19)

These tasks belong to [zero-access plan 17: JMAP calendars and browser sign-in](2026-11-14-zero-access-17-jmap-calendars.md), whose header holds the commands, the constraints and the task index. "I §n" is section n of the [interface sheet](2026-11-14-zero-access-17-jmap-calendars-interfaces.md).

Spec sections 4.2 to 4.9 and the token rows of sections 8, 9 and 10. The sheet sections these tasks cite are I §5 (tokens, with the fixed trace texts in I §5.6), I §9 (hooks) and I §10 (test helpers and fixtures).

---

### Task 13: Vault primitives: secrets, the record, the hand-off value

**Spec:** 4.2 (the secret, the derivations, the associated data, D26), 4.3 (the record, its caps, its reconciliation, D19, D20), 4.4 (the hand-off value), 4.5 step 2 (the classification), 9.1.

**Files:**
- Modify `crates/vault/Cargo.toml`: add `encodify = "1.0"`, the base64 crate the workspace uses. `Cargo.lock` follows.
- Modify `crates/vault/src/keys.rs`: labels after `:30`; derivations after `derive_search_key` (`:162-164`); `ZaTokenKind` and the three associated-data functions after `app_aad` (`:199-201`); tests in the module at `:419`.
- Modify `crates/vault/src/record.rs`: `za_session_binding` after `impl VaultRecord` (`:80-139`); its test in the module at `:141`.
- Create `crates/vault/src/tokens.rs`.
- Modify `crates/vault/src/lib.rs:7-12`: `pub mod tokens;`.

**Interfaces:**
- Consumes, existing in `crates/vault/src/keys.rs`: `Secret` (`:33-55`), the private `hkdf_expand` (`:118-124`), `aad` (`:188-195`), `wrap_key` (`:244-246`), `unwrap_key` (`:248-255`).
- Produces: `za_session_binding` (I §5.1); all of I §5.2; all of I §5.3.

**Constraints:**
- A secret's text is decoded straight into a `Zeroizing<[u8; 32]>` (`URL_SAFE_NO_PAD.decode_slice`), never through a plain `Vec`.
- Every derivation is `hkdf_expand(secret.as_bytes(), LABEL_…)`. Every wrap is `wrap_key`, 72 bytes.
- An entry opens only for the account, grant, kind and, for a refresh entry, the generation it was made for (invariant 63): all four are in the associated data, never only in the record's fields. A hand-off opens only for its account, binding and key id, the same way.
- `VaultRecord`'s layout does not change. No existing label or associated-data purpose changes.
- No method adds a refresh entry it was not asked for (invariant 67); `reconcile` only removes.
- Nothing in this crate logs.

- [ ] **Step 1: write the failing tests.** `mk`, `s0`, `s1`, `s2`, `refresh` are `Secret::random()` unless a value is given.

  `record::tests::binding_changes_only_with_credentials`, on `full_record()`:
  - `za_session_binding(&record)` is not 0 and is the same on two calls.
  - It changes, each time from the same base record, when one byte of `salt` changes, when `argon2_m_cost_kib`, `argon2_t_cost` or `argon2_p_cost` goes up by one, when one byte of `verifier_hash` changes, when `totp_url` goes from `Some` to `None`, and when one byte of the URL changes: seven values, all different from the base's and from each other.
  - It does not change when `revision`, `state`, `password_wrap`, `recovery_wrap`, `app_wraps`, `public_key`, `private_key_wrap`, `setup_token_hash` or `setup_token_expires` changes.
  - The framing is unambiguous: `salt = [1, 2]` with `verifier_hash = [3]` and `salt = [1]` with `verifier_hash = [2, 3]` give different values; so do `totp_url = None` and `totp_url = Some("")`.
  - The construction is I §5.1's: for the base record the value equals the first 8 bytes, big-endian, of SHA-256 over `b"za/v1/session-binding"` and the fields as the test writes them out by hand.

  `keys::tests::token_labels_and_associated_data_are_fixed`, with `secret = Secret::from_bytes([9; 32])`:
  - `derive_token_wrap_key`, `derive_token_next_key` and `derive_handoff_key` equal `hkdf_expand(secret.as_bytes(), label)` for `b"za/v1/token/wrap"`, `b"za/v1/token/next"` and `b"za/v1/handoff"`.
  - `derive_token_tag(&secret)` is the first 8 bytes of the same for `b"za/v1/token/tag"`. The wrap key differs from the next key.
  - `token_aad(7, 23, ZaTokenKind::Access) == b"za/v1|token/23/access|\0\0\0\x07"`
  - `token_aad(7, 23, ZaTokenKind::Refresh { generation: 4 }) == b"za/v1|token/23/refresh/4|\0\0\0\x07"`
  - `token_next_aad(7, 23, 4) == b"za/v1|token/23/next/4|\0\0\0\x07"`
  - `handoff_aad(7, 12, "c0de") == b"za/v1|handoff/12/c0de|\0\0\0\x07"`

  The rest are in `tokens::tests`.

  `token_entry_binds_account_grant_kind_generation`. A record `new(3)` with `put_refresh(7, 23, &mk, None, 0, &refresh, 1_000)` and `access = put_access(7, 23, &mk, 1_000)`:
  - `open_access(7, 23, &access, 10)` is MK; `classify_refresh(7, 23, 0, &refresh, 10)` is `Opened { next: None, .. }`.
  - Another account: `open_access(8, 23, &access, 10)` is `None`; `classify_refresh(8, 23, 0, &refresh, 10)` is `Lost`.
  - Another grant (a clone with `grants[0].grant_id = 24`): `open_access(7, 24, &access, 10)` is `None`; `classify_refresh(7, 24, 0, &refresh, 10)` is `Lost`.
  - Another kind (a clone whose access entry takes the refresh entry's `wrap` and the tag `derive_token_tag(&refresh)`): `open_access(7, 23, &refresh, 10)` is `None`.
  - Another generation (a clone whose refresh entry is relabelled generation 1): `classify_refresh(7, 23, 1, &refresh, 10)` is `Lost`.

  `split_secret_cases`, every call with the header `"sw1."`. `token` is `"sw1.AAAA.BBBB"` after `append_secret` of `Secret::from_bytes([0xA5; 32])`:
  - `token.len() == 13 + 1 + SECRET_TEXT_LEN`; byte 13 is `~`; the 43 characters after it are ASCII letters, digits, `-` or `_`.
  - `split_secret("sw1.AAAA.BBBB", "sw1.")` is `("sw1.AAAA.BBBB", None)`.
  - `split_secret(&token, "sw1.")` is `("sw1.AAAA.BBBB", Secret(s))` with `s` the secret's bytes.
  - `Malformed`, each with the upstream part `"sw1.AAAA.BBBB"`:

    | Input | Why |
    |---|---|
    | `"sw1.AAAA.BBBB~"` | empty |
    | `token` less its last character | 42 characters |
    | `token` plus `A` | 44 characters |
    | `token` with its last character replaced by `=` | padding |
    | the same with `+` | the standard alphabet |
    | `token` with `~` doubled | a second `~` |
    | `token`, `~`, and the 43 characters again | a second suffix |
    | `token` with its last character replaced by `_` | trailing bits `11`, not canonical |
  - No header: `split_secret("opaque~value", "sw1.")` is `("opaque~value", None)`, and so is `"opaque~"` followed by the 43 characters of `token`'s suffix, returned whole.

  `split_code_cases`. `code` is `"a"` 40 times; `with` is `code` after `append_code_secret` of `Secret::from_bytes([0x5A; 32])`:
  - `split_code(&code)` is `(code, None)`.
  - `with.len() == CODE_LEN + SECRET_TEXT_LEN` and it holds no `~`; `split_code(&with)` is `(code, Secret(s))` with the secret's bytes.
  - `Malformed`, the whole input returned as the first part: `code[..39]`; `with[..82]`; `with` plus `A`; `code`, `~` and the 43 characters; `with[..82]` plus `=`; 83 bytes with no character boundary at 40 (`code[..39]`, `é`, 42 characters of the secret); the empty string.

  `record_round_trips_and_caps`:
  - `new(5)`: `is_empty()`, `version == TOKEN_RECORD_VERSION`, `binding == 5`.
  - `put_refresh(7, 23, ..)` for generation 0 (expires 100), 1 with `Some((0, &s0))` (200) and 2 with `Some((1, &s1))` (300): the grant's generations are `[1, 2]`.
  - `put_access(7, 23, &mk, expires)` for 50, 10, 40, 30, 20: four entries, none with `expires == 10`; the second secret opens nothing and the other four open at `now = 0`.
  - Every `wrap` is 72 bytes; generation 1's `next` is 72 bytes.
  - `rkyv::to_bytes` then `rkyv::from_bytes` gives an equal record.
  - `remove_grant(23)` is true, a second call false, then `is_empty()`.
  - A full record (32 grants, each with generations 0 and 1, the first sealing the second, and four access entries) archives to fewer than 50,000 bytes (D20's reason).

  `reconcile_cases`. Grants 1 to 6 of account 7, each with a refresh entry of generation 0 and an access entry, all expiring at 1,000; grant 4 also holds generation 1; grant 5's entries expire at 500. `reconcile(7, 500, &grants)` with:

  | Grant | Its state in the map | After |
  |---|---|---|
  | 1 | account 7, generation 0 | kept, unchanged |
  | 2 | `None` | dropped |
  | 3 | account 8, generation 0 | dropped |
  | 4 | account 7, generation 1 | kept; generations `[1]` |
  | 5 | account 7, generation 0 | dropped: no entry left |
  | 6 | absent from the map | dropped |

  It returns true; the grant ids are `[1, 4]`; a second call returns false.

  `classify_refresh_cases`, grant 23 of account 7, entries expiring at 100:
  - Generation 0 alone: `classify_refresh(7, 23, 0, &s0, 10)` is `Opened { mk, next: None }` with MK's bytes.
  - After `put_refresh(.., Some((0, &s0)), 1, &s1, 100)`: `Opened` with `next: Some(s1)`.
  - A clone without entry 0, presented 0: `Superseded`.
  - `Lost`: an empty record; that clone presented 2 (gone, nothing higher); a random secret on generation 0; generation 1 with `&s1` at `now = 100` (expired, nothing higher); entry 0 with `next` replaced by 72 zero bytes.

  `put_refresh_restores_missing_next`:
  - Generations 0 and 1 as above; entry 1 removed; the retry `put_refresh(.., Some((0, &s0)), 1, &s1, 300)`: entry 0's `next` is byte for byte what it was, and `classify_refresh(7, 23, 1, &s1, 10)` is `Opened { next: None, .. }`.
  - The same call again with expiry 999 leaves the record equal.

  `eviction_picks_least_recently_used`:
  - Grants 1 to 31 with `last_used_at = 1_000 + id`: `eviction(99, ..)` is `None`.
  - Grant 32 added with `last_used_at = 5`: `eviction(32, ..)` is `None` and `eviction(99, ..)` is `Some(32)`.
  - Grant 9's `last_used_at` set to 5: `eviction(99, ..)` is `Some(9)`, the lowest id on a tie.
  - `MAX_KEYED_GRANTS == 32`.

  `handoff_opens_only_as_sealed`. `(secret, value) = seal_handoff(&mk, 7, 12, key)` with `key` = `"c"` 40 times:
  - `value.len() == 85`; `value[0] == HANDOFF_VERSION`; `value[1..5] == 7u32.to_be_bytes()`; `value[5..13] == 12u64.to_be_bytes()`.
  - `open_handoff(&value, &secret, key)` is `(7, 12, mk)`.
  - `None` for: another secret; the key id `"d"` 40 times; `value[4] ^= 1` (another account); `value[12] ^= 1` (another binding); `value[0] = 2`; `value[..84]`.

- [ ] **Step 2: run them.** U-vault with the name `token`, then with `binding_changes_only_with_credentials`. Expected: the crate does not compile; the errors name `za_session_binding`, `derive_token_wrap_key` and `ZaTokenRecord`.

- [ ] **Step 3: implement `za_session_binding`** (I §5.1). Compute the credential fingerprint exactly as the sheet specifies, including field framing, the first-eight-byte big-endian result and the zero-to-one rule. Read no other vault field; every dependent place goes through this function (D19).

- [ ] **Step 4: implement the additions of I §5.2.** The associated-data functions call `aad` as `app_aad` does. The hand-off's purpose is `handoff/{binding}/{key_id}`.

- [ ] **Step 5: implement I §5.3.** Rules:
  - **Wraps.** An access entry's `wrap` is `wrap_key(mk, &derive_token_wrap_key(secret), &token_aad(account, grant, ZaTokenKind::Access))` and its `tag` is `derive_token_tag(secret)`. A refresh entry's `wrap` uses `ZaTokenKind::Refresh { generation }`. A sealed next secret is `wrap_key(next_secret, &derive_token_next_key(presented_secret), &token_next_aad(account, grant, presented_generation))`.
  - **The hand-off.** `seal_handoff` draws a fresh `Secret::random()` and wraps MK with `wrap_key(mk, &derive_handoff_key(&secret), &handoff_aad(account_id, binding, key_id))`. `open_handoff` requires 85 bytes and `HANDOFF_VERSION`, reads the account id and the binding from the value, passes both to the associated data, and returns `None` on anything else.
  - **`split_secret(token, header)`.** A value that does not begin with `header` is returned whole with `None`, whatever it holds. No `~`: `(token, None)`. Otherwise the upstream part is everything before the first `~`; the rest is a secret only if it is exactly 43 characters that `encodify::base64::URL_SAFE_NO_PAD` decodes strictly to 32 bytes; anything else is `Malformed`.
  - **`split_code`.** 40 bytes: `(code, None)`. 83 bytes with a character boundary at 40 whose last 43 decode as above: `(&code[..40], Secret)`. Anything else: `(code, Malformed)`, the whole input.
  - **`put_access`.** Creates the grant when the record lacks it, appends the entry, then, past four entries, removes the one with the smallest `expires`, the earliest in the list on a tie.
  - **`put_refresh`.** Creates the grant when the record lacks it. Adds generation `next`'s entry (`next: None`) only when it is absent. With `presented = Some((g, secret_g))`, seals `next_secret` into entry `g` only when that entry exists and its `next` is `None`. Then keeps the two highest generations. Refresh entries stay in ascending order of generation; access entries and grants stay in insertion order.
  - **`reconcile`.** Drops a grant whose id is absent from `grants`, maps to `None`, or maps to a state of another account. For a kept grant drops every refresh entry with `generation < state.generation` and every entry with `expires <= now`. Drops a grant left with no entry. Returns whether anything changed.
  - **`eviction`.** `None` when the record names `adding` or holds fewer than 32 grants; otherwise the record's grant with the smallest `(last_used_at, grant_id)`, a grant without a state counting as the oldest.
  - **`open_access`.** The grant's access entry whose `tag` matches and whose `expires > now`, unwrapped; `None` otherwise.
  - **`classify_refresh`.** An entry counts only while `expires > now`. The presented generation's entry unwraps: `Opened`, with `next` unwrapped when the entry holds one; a `next` that does not unwrap is `Lost`. No entry of the presented generation and an entry of a higher one: `Superseded`. Everything else: `Lost`.

- [ ] **Step 6: run the tests.** U-vault with the same two names. Expected: the eleven new tests pass.

- [ ] **Step 7: the closing checks.**

- [ ] **Step 8: commit.** `Release 6: token secrets, the token record and the hand-off value`

Catchers for Task 42: "the wrap's associated data omits the grant id" is `token_entry_binds_account_grant_kind_generation`; "a superseded refresh entry is kept" is `reconcile_cases`.

---

### Task 14: Field 153, storage, `za_token_read`, the vault write's deletion

**Spec:** 4.3 (the field, the two assertions, D19, D21), 4.7 (a vault write), invariants 64 and 69.

**Files:**
- Modify `crates/types/src/field.rs`: the variant after `:98`, the `u8` arm after `:214`.
- Modify `crates/common/src/za/mod.rs:14-18`: `pub mod tokens;`.
- Create `crates/common/src/za/tokens.rs` (the read, the write and `za_token_read` only).
- Modify `crates/common/src/auth/vault.rs:197-239` (`vault_batch_write`) and its `tests` module (`:1489`).
- Modify `crates/common/src/auth/access_token.rs:183`.
- Modify `tests/src/utils/cleanup.rs:258-262` and `:484-503`, `tests/src/utils/storage.rs:373`, `tests/src/utils/mod.rs` (`pub mod oauth;`), `tests/src/utils/za.rs` (`pub use crate::utils::oauth::*;`).
- Create `tests/src/utils/oauth.rs` (the three record helpers).
- Create `tests/src/za/tokens.rs`; modify `tests/src/za/mod.rs`: `pub mod tokens;` in the list at `:16-53`, and `tokens::test(&mut test).await;` immediately after `sharing_races::test` and before Task 10's `jmap_calendar::pipeline::test`.

**Interfaces:**
- Consumes: `za_session_binding` (I §5.1); `ZaTokenRecord`, `TOKEN_RECORD_VERSION` (I §5.3). Existing in `crates/common/src/auth/vault.rs`: `RawValue` (`:46-52`), `VaultRead` (`:54-58`), `vault_read_store` (`:106-133`), `vault_write_store` (`:152-170`), `ZaPinRead::assertion` (`:266-270`, the model for `ZaTokenRead::assertion`).
- Produces: `PrincipalField::ZeroAccessTokens = 153`; `ZaTokenRead`, `tokens_key`, `token_read_store`, `token_write_store`, `Server::za_token_read` (I §5.4); the first two rows of I §5.1's table; `raw_token_record`, `set_raw_token_record`, `token_record` (I §10).

**Constraints:**
- The stored value is one byte `TOKEN_RECORD_VERSION` followed by `Archiver::with_compression(record.clone(), Compression::None)`, as the vault record is stored (`vault.rs:205-212`). The read is a checked decode, never the unchecked access (`vault.rs:123-128`).
- A value that does not decode, or whose version is not 1, reads as `record: None` with `raw` kept. It raises no error and no trace: the store's own deserialization errors would carry the stored bytes.
- A write that stores a wrap asserts the vault record's bytes and field 153's previous bytes (invariant 64). With `vault_cas: None`, `token_write_store` returns `StoreEvent::UnexpectedError` with its text of I §5.6, writing nothing, unless every grant and entry of `record` is in `previous.record` unchanged (invariant 69).
- A vault write that changes the binding deletes the token record in its own batch (invariant 64): a password change, a recovery or a second-factor change, and no other vault write (spec D19).
- An existing za assertion that a key account's OAuth token or grant ends after a vault write that changes no credential (a new recovery key, an app-password write, the self-check's repair, a first pin) changes to "still served", with a comment citing spec D19. Find them before Step 10 with `git grep -n -i 'invalid_grant\|TokenExpired\|oauth' tests/src/za`, and list each in the task report. None is changed silently.
- `vault_delete_store` is not touched: it removes pending records only.

- [ ] **Step 1: write the failing unit tests.** Each runs on `EphemeralStore::open()` with account 5.

  In `crates/common/src/za/tokens.rs`, module `tests`. The vault record is `VaultRecord::pending(vec![1; 32], 100)` written with `vault_write_store(.., None)`; a "record of grants g…" is `ZaTokenRecord::new(1)` with `put_refresh(5, g, &mk, None, 0, &Secret::random(), 1_000)` for each.

  `token_record_round_trips_and_asserts`:
  - Absent: `raw` and `record` are `None`; `assertion()` is `AssertValue::None`.
  - A write of a record of grant 9 with `Some(vault.cas)` on that read succeeds. The read back equals it, `raw[0] == TOKEN_RECORD_VERSION` and `assertion()` is `Hash(xxh3_64(raw))`.
  - A second write on the stale (absent) read is an assertion failure and the stored record is unchanged.
  - A write of the empty `ZaTokenRecord::new(1)` with `None` on the fresh read leaves the field absent.

  `record_write_loses_to_vault_write`:
  - A record written under the vault read `before`; then `vault_write_store` on `before` of the same record at `revision + 1`, a vault write that leaves the binding unchanged: field 153 still holds the record (spec D19).
  - A write of the token record with `Some(before.cas)` on a fresh read of field 153 is an assertion failure and the stored record is unchanged: a writer that read the vault before a vault write cannot store after it without reading again.
  - Then `vault_write_store` with `verifier_hash` changed, a credential change: field 153 is absent.

  `removal_write_asserts_the_record_alone`:
  - A record of grants 9 and 10 written under the vault read `before`; then field 150 set raw to `[VAULT_RECORD_VERSION, 0xff]`, with no vault write.
  - The removal of grant 9 with `Some(before.cas)` is an assertion failure (the control).
  - The same removal with `None` succeeds and reads back.

  `removal_write_cannot_add_an_entry`, with `vault_cas: None` on the read of a stored record of grant 9:
  - That record plus one access entry, a record of grants 9 and 10, and another record of grant 9 (other wraps) each fail with `StoreEvent::UnexpectedError`.
  - A non-empty record on an absent read fails.
  - The stored record is unchanged.

  `unusable_bytes_read_as_no_record`, for each of `[1, 0xde, 0xad]`, `[2, 0, 0]` and `[1]` set raw in field 153:
  - `record` is `None`; `raw` is the bytes; `assertion()` is `Hash(xxh3_64(bytes))`.
  - A write with `Some(vault.cas)` on that read succeeds.

  In `crates/common/src/auth/vault.rs`, module `tests`, with `[1, 0xde, 0xad]` planted raw in field 153:

  `credential_change_clears_the_token_record`:
  - After `vault_commit_store(.., previous: None, ..)`: absent (a first vault write has no earlier binding).
  - Planted again; after a commit on the fresh read that changes only `revision`: still `[1, 0xde, 0xad]`. The same after a commit that changes `recovery_wrap`, `app_wraps`, `public_key` or `private_key_wrap`.
  - After a commit that changes `verifier_hash`: absent. Planted again each time, the same after a change of `salt`, of an Argon2 parameter and of `totp_url`.

  `a_lost_vault_write_keeps_the_token_record`:
  - Two commits, the second on the read `stale`; the plant; a third commit on `stale` is an assertion failure and field 153 still holds `[1, 0xde, 0xad]`. The ephemeral store applies whatever precedes a failed assertion, so this fails if the clear is placed before one.

- [ ] **Step 2: run them.** U-common with `za::tokens`. Expected: the crate does not compile; `PrincipalField` has no variant `ZeroAccessTokens`.

- [ ] **Step 3: add field 153** to `crates/types/src/field.rs`: the variant `ZeroAccessTokens = 153` after `ZeroAccessSearch`, with a doc comment in the style of `:89-97` ("Zero-access token record (release 6 spec 4.3). 153 has never been assigned to any field in upstream's history."), and the arm `PrincipalField::ZeroAccessTokens => 153`.

- [ ] **Step 4: implement the store functions and `za_token_read`** (I §5.4). `tokens_key` is `ValueKey::property(account_id, Collection::Principal, 0, PrincipalField::ZeroAccessTokens)`. The batch selects the account, `Collection::Principal` and document 0, then, in this order: the vault assertion (`AssertValue::Hash(vault_cas)`, only with `Some`), the assertion of field 153 (`previous.assertion()`), the set, or the clear when `record` is `None` or empty.

- [ ] **Step 5: delete the record in the vault write.** In `vault_batch_write`, after the last assertion (`:230`) and before the first set (`:231`), add `batch.clear(PrincipalField::ZeroAccessTokens)` when `previous` is `None` or `za_session_binding(&previous.record) != za_session_binding(record)`. Extend the function's comment (`:194-196`) to name the token record and spec 4.7.

- [ ] **Step 6: the credential version.** At `crates/common/src/auth/access_token.rs:183`, replace `read.record.revision` with `za_session_binding(&read.record)`. The `.unwrap_or(0).max(1)` stays.

- [ ] **Step 7: run the unit tests.** U-common with `za::tokens`, then with `auth::vault::tests`. Expected: the seven new tests pass and the existing vault tests are unchanged.

- [ ] **Step 8: the test helpers.**
  - `tests/src/utils/cleanup.rs:484-503`: add `|| key[U32_LEN + 1] == PrincipalField::ZeroAccessTokens as u8` to the exemption, and "token" to its comment and to the message at `:258-262` ("vault, pin, search or token state").
  - `tests/src/utils/storage.rs:373`: "a zero-access vault, pin, search or token record".
  - `tests/src/utils/oauth.rs`: `raw_token_record` and `set_raw_token_record` as `raw_pins` and `set_raw_pins` are written (`tests/src/utils/za.rs:3030-3051`), for field 153; `token_record` returns `test.server.za_token_read(account_id).await.unwrap().record`.

- [ ] **Step 9: write the za cases.** `tests/src/za/tokens.rs` prints `Running zero-access token tests...`, creates the key accounts `token-owner` (`OWNER`) and `token-mfa` (`MFA`) with `create_key_user_account(.., STRONG, .., &[], user_permissions())` and `test.insert_account`, runs the cases in the order below and ends with `test.wait_for_tasks()`. `PLANTED` is `[1, 0xde, 0xad]`, set with `set_raw_token_record`: the clear does not read what the field holds. `STRONG2` is `"another long passphrase with 2 numbers"`.

  `pin_rewrap_keeps_session`, first because `OWNER` has never pinned; `lookup`, `pin` and `forget` are `pinning`'s (`tests/src/za/pinning.rs:4780-4812`):
  - `wrap_mark(test, id, STRONG)` is `ZaMark::Legacy`; `PLANTED` set.
  - `pin(OWNER, "key1@example.com", &fingerprint)` answers 200, with the fingerprint of `lookup(..).str(200, "fingerprint")`. `wrap_mark` is `Pinned`, the vault revision is one higher, and `raw_token_record` is still `Some(PLANTED)` (spec D19: the first pin's rewrap is a vault write that changes no credential).
  - A pin of `MFA` answers 200; `raw_token_record` is `Some(PLANTED)` (a later pin writes field 151 alone).
  - The field is cleared and both pins are forgotten (200 each).

  `credential_changes_end_sessions` and `other_vault_writes_keep_sessions`, two cases over one table. For each row: `PLANTED` set, the write made, the vault revision one higher, then `raw_token_record(test, id)` is `None` in a row of the first case and `Some(PLANTED)` in a row of the second. The first case holds the rows password, recover, second factor enrolled and second factor removed. The second holds recovery key, the four app-password rows and the self-check's repair. Request bodies are those of `tests/src/za/password.rs:168-173`, `:242-247`, `tests/src/za/totp.rs:230-233`, `:363-368` and `tests/src/za/app_password.rs:59-80`.

  | Row | The write |
  |---|---|
  | password | `za_post("password", {username, password: STRONG, new_password: STRONG2})`, 200 |
  | recover | `za_post("recover", {username, recovery_key: owner.recovery_key, new_password: STRONG})`, 200 |
  | recovery key | `za_post("recovery-key", {username, password: STRONG})`, 200 |
  | second factor, enrolled | `za_post("totp", {username, password, otp_auth: url, confirm: code(&url)})`, 200 |
  | second factor, removed | `za_post("totp", {username, password, otp_auth: null, totp: code(&url)})`, 200 |
  | app password, pending | a creation parked with `Parked::start(id, za_test::set_publish, za_post("app-password", ..))`; planted before it; asserted while it is parked |
  | app password, published | planted while it is parked; `parked.finish()` answers 200 |
  | app password, revoked | `za_post("app-password/revoke", {username, password, credential_id})`, 200 |
  | app password, withdrawn | a second creation parked at `za_test::set_publish`; the account's registry object edited while it is parked (a private helper written as `bump_registry_revision`, `tests/src/za/app_password.rs:162-195`, is); planted; `finish()` answers 409 with `"registry write failed"` |
  | the self-check's repair | under `DrainOn::new()`: `substitute_public_key(test, id, &writer_key(), true)`, planted, `trigger_drain(OWNER, STRONG)` answers 207, and the stored public key is the original again (`tests/src/za/pinning.rs:262-272`) |

  The cases end with the account at `STRONG`, no second factor and no app password; the second case clears the field at its end.

- [ ] **Step 10: run the suite.** ZA. Expected: the banner line of `tokens`, then the pass line. `setup` (`test_grants_die_with_the_marker`), `app_password`, `caches` and `pinning` exercise the credential version and the vault batch. Then SWAP.

- [ ] **Step 11: the closing checks.**

- [ ] **Step 12: commit.** `Release 6: the token record's field and its deletion by a vault write`

Catchers for Task 42: "the vault commit leaves the token record" is `credential_changes_end_sessions` and `credential_change_clears_the_token_record`; "a token-record write omits the vault assertion" is `record_write_loses_to_vault_write` (U-common), then Task 17's `exchange_crossed_by_password_change` (ZA).

---

### Task 15: Issue, open, forget; the grant hook for key accounts

**Spec:** 4.3 (the write loop, reconciliation, D20, D21), 4.4 (the first entries), 4.6 (the open), 4.7 (a grant deletion).

**Files:**
- Modify `crates/common/src/za/tokens.rs`.
- Modify `crates/common/src/za/test_hooks.rs`: the module table (`:7-100`), the statics beside `:169` and `:189`, the setters beside `:243-245` and `:292-294`, the points beside `:460-461` and `:552-554`.
- Modify `crates/common/src/auth/oauth/grant.rs:385-392`.
- Test: `tests/src/za/tokens.rs`.

**Interfaces:**
- Consumes: the record methods and `ZaGrantState` (I §5.3); `token_read_store`, `token_write_store` (I §5.4); `za_session_binding` (I §5.1). Existing: `vault_read_usable_store` (`crates/common/src/auth/vault.rs:139-150`), `Server::za_is_key_account` (`vault.rs:463`), `Server::oauth_grant` (`grant.rs:160-175`; its `account_id()`, `grant.generation`, `grant.last_used_at.timestamp()`), `Server::delete_oauth_grant` (`grant.rs:369-419`), `SessionKeys::new` (`crates/vault/src/session.rs:168-185`).
- Produces: `ZaIssued`, `ZaTokenIssue`, `za_token_issue`, `za_token_open_access`, `za_token_forget_grant` (I §5.4); `TOKEN_EXCHANGE_PAUSE` and `TOKEN_FORGET_FAIL_ONCE` with their setters and points (I §9); the row for `grant.rs` in I §5.5.
- Beyond the sheet, private to `tests/src/za/tokens.rs`: `minted(test, name, id, password)` returns a grant id, upstream's access token for it, the account's keys and its binding now (`za_grant_token(&test.server, name, OAuthScopes::ALL)`, `za_unlock`, `za_session_binding` of the vault record). `HOUR` is 3,600 and `MONTH` 2,592,000.

**Constraints:**
- `za_token_issue` stores wraps only under the binding it was given. A stored record of another binding is not extended: the write starts from `ZaTokenRecord::new(binding)` (invariant 64).
- Its write asserts the vault record's bytes and field 153's bytes as read in the same attempt.
- `za_token_forget_grant` reads no vault record and asserts field 153 alone (D21, invariant 69).
- Every write reconciles first (spec 4.3): one `oauth_grant` read per grant the record names.
- The record never holds more than 32 grants (invariant 70). The evicted grant's entries leave in the write; the grant object is deleted after the write commits.
- The hook reads field 153 only for a key account: an ordinary account pays one cached classification read at a grant's deletion (spec 4.7). The hook never changes `delete_oauth_grant`'s result.
- `rotate_oauth_grant` and upstream's prune are not touched.
- Every case revokes the grants it minted.

- [ ] **Step 1: write the failing za cases.** Add the key account `token-cap` (`CAP`) and the ordinary account `token-plain` (`PLAIN`, `create_user_account`; the module destroys it at its end, after `wait_for_index_queue`). `id` is `OWNER`'s. "Issued for g" means `minted`, then `za_token_issue(id, g, keys.mk(), binding, now() + HOUR, now() + MONTH)` answering `Issued(issued)`.

  `issue_writes_first_entries`, issued for `grant`:
  - The record has `version == TOKEN_RECORD_VERSION`, `binding` equal to the account's and one grant.
  - Its grant holds one refresh entry, generation 0 with `next` `None`, and one access entry whose `tag` is `derive_token_tag(&issued.access)`.
  - `record.open_access(id, grant, &issued.access, now())` is the keys' MK; `record.classify_refresh(id, grant, 0, &issued.refresh, now())` is `Opened { next: None, .. }`.

  `issue_refuses_changed_binding`:
  - `minted` gives the binding `stale`; then `za_post("recovery-key", {username: OWNER, password: STRONG})` answers 200, a vault write that changes no credential: `za_token_issue(.., stale, ..)` is still `Issued` and the record's `binding` is `stale`. The grant is revoked.
  - `minted` again; then a password change to `STRONG2` answers 200: `za_token_issue(.., stale, ..)` is `VaultChanged` and `raw_token_record` is `None`. The password goes back to `STRONG`.

  `open_access_needs_entry_and_binding`, issued for `grant` and for `other`:
  - `za_token_open_access(id, grant, &issued.access)` is `Some(keys)` with `generation == vault_revision(test, id)`, `account_id == id` and MK's bytes.
  - `None` for a random secret, for `(other, &issued.access)` and for `(grant, &issued.refresh)`.
  - After a vault write that changes no credential (`recovery-key`, 200): still `Some(keys)`, with `generation` one higher (spec D19: the keys are opened again at the new revision).
  - After a password change to `STRONG2` (200): `None`, the record is gone. The password goes back to `STRONG` at the case's end.
  - The record's earlier raw bytes put back with `set_raw_token_record`: still `None`, since their binding is no longer current. Then the field is cleared.

  `forget_grant_asserts_the_record_alone`, issued for `g1` and `g2`:
  - Field 150's raw bytes are saved and set raw with their last byte flipped: the vault record is one no reader can use, with no vault write.
  - `za_token_forget_grant(id, g1)` succeeds; the record lacks `g1` and holds `g2`.
  - A second `za_token_forget_grant(id, g1)` succeeds and leaves `raw_token_record` byte for byte (no write when the record lacks the grant).
  - Field 150 is restored and `za_invalidate_account(id)` called.

  `revocation_removes_entries`, issued for `g1` and `g2`:
  - `za_revoke_grant(&test.server, g1)`: the record lacks `g1`.
  - `za_revoke_grant(&test.server, g2)`: `raw_token_record` is `None`.

  `cap_evicts_least_recently_used`, on `CAP`:
  1. 32 grants minted and issued, `grants[0]` first: the record holds 32.
  2. A sleep of 1,100 ms; then `grants[0]` is used: `rotate_oauth_grant(oauth_grant(grants[0]), 0)` is `Issue { .. }`, which stamps `lastUsedAt`.
  3. A 33rd grant `last` is minted and issued: `Issued`.
  4. The record holds 32 grants, lacks `grants[1]`, and holds `grants[0]` and `last`. `oauth_grant(grants[1])` is `None`; `oauth_grant(grants[0])` is `Some`.
  5. The 32 grants that remain are revoked; the field is absent.

  `forget_failure_does_not_fail_deletion`, issued for `g`:
  - `test_hooks::set_token_forget_fail_once(Some(id))`, then `za_revoke_grant(&test.server, g)`, which asserts that `delete_oauth_grant` returned `Ok(true)`.
  - `oauth_grant(g)` is `None`; the record still names `g`.
  - `za_token_forget_grant(id, g)` then succeeds and `raw_token_record` is `None`.

  `ordinary_grant_deletion_reads_no_record`, on `PLAIN`. An ordinary account never holds the field; the plant and the hook are the probes.
  - `PLANTED` set in `PLAIN`'s field 153 and `set_token_forget_fail_once(Some(plain_id))`.
  - A grant of `PLAIN` minted with `za_grant_token` and revoked with `za_revoke_grant`.
  - `raw_token_record(test, plain_id)` is `Some(PLANTED)`, and `test_hooks::take_token_forget_fail(plain_id)` is still true: `za_token_forget_grant` was not entered.
  - The field is cleared.

- [ ] **Step 2: run them.** TM. Expected: the `tests` crate does not compile; `za_token_issue` and `set_token_forget_fail_once` are not found.

- [ ] **Step 3: add the two hooks of I §9** to `crates/common/src/za/test_hooks.rs` in the file's style (`set_slot`, `take_for`, `wait`, `take_flag`), each with a table line that repeats its place: `TOKEN_EXCHANGE_PAUSE` in `za_token_issue` after the binding check and before the batch; `TOKEN_FORGET_FAIL_ONCE` at the head of `za_token_forget_grant`.

- [ ] **Step 4: implement `za_token_issue`.** Each of at most three attempts runs these steps in order; an attempt ends early with the result shown.
  1. Read the vault record (`vault_read_usable_store`). None, or `za_session_binding(&read.record) != binding`: `VaultChanged`. A vault write that left the binding unchanged passes this check, so an attempt that lost its vault assertion to one is made again.
  2. Read field 153. Start from its record when it is usable and its `binding` equals `binding`; otherwise from `ZaTokenRecord::new(binding)`.
  3. Read `oauth_grant` for `grant_id` and for every grant the record names. `grant_id` missing or of another account: a `StoreEvent::UnexpectedError` with the details `zero-access: OAuth grant not found`, the account id and the grant id, which the token endpoint answers as a server error.
  4. `reconcile(account_id, now(), &grants)`.
  5. `eviction(grant_id, &grants)`; with `Some(evicted)`, `remove_grant(evicted)`.
  6. `put_refresh(account_id, grant_id, mk, None, 0, &Secret::random(), refresh_expires)` and `put_access(account_id, grant_id, mk, access_expires)`.
  7. Under `test_mode`: `token_exchange_pause_point(account_id)`.
  8. `token_write_store(store, account_id, Some(vault.cas), &read, Some(&record))`. An assertion failure starts the next attempt; after the third it is returned. Any other error is returned.
  9. After the write, delete the evicted grant: `oauth_grant(evicted)`, then `delete_oauth_grant(stored, GrantDeletion::Unconditional, ..)` and `invalidate_caches`. A failure is logged with its text of I §5.6 and not returned. Return `Issued` with the two secrets of step 6.

- [ ] **Step 5: implement `za_token_open_access`.** Read field 153; `record.open_access(account_id, grant_id, secret, now())`; read the vault record (`vault_read_usable_store`); require `record.binding == za_session_binding(&vault.record)`; return `SessionKeys::new(account_id, vault.record.revision, mk)` in an `Arc`. Every failed condition is `Ok(None)`. It writes nothing.

- [ ] **Step 6: implement `za_token_forget_grant`.** Under `test_mode`, first: when `take_token_forget_fail(account_id)`, return a `StoreEvent::UnexpectedError` and read nothing. Then at most three attempts: read field 153; no usable record, or one that lacks the grant, returns `Ok(())` without a write; `remove_grant`; reconcile as steps 3 and 4 of `za_token_issue` do; `token_write_store(.., None, &read, Some(&record))`. An assertion failure starts the next attempt; after the third it is returned.

- [ ] **Step 7: the hook in `delete_oauth_grant`** (`crates/common/src/auth/oauth/grant.rs:385-392`), in the `Success` arm before `return Ok(true)` and in the `NotFound` arm before `return Ok(false)`:
  1. `self.za_is_key_account(account_id)`. `Ok(false)`: nothing is read.
  2. `Ok(true)`: `self.za_token_forget_grant(account_id, stored.id)`.
  3. An `Err` of either call is logged with `trc::error!`, the account id, the grant id and the hook's text of I §5.6, and is never returned (spec 4.7: the sweep bounds it).

- [ ] **Step 8: run the suite.** ZA. Expected: the `tokens` module prints the eight case names, then the pass line. `setup`, `dav_gate`, `caches` and `mail::gates` delete grants through the hooked function.

- [ ] **Step 9: the closing checks.**

- [ ] **Step 10: commit.** `Release 6: issue, open and forget token-record entries`

Catchers for Task 42: "the cap's eviction leaves the evicted grant" is `cap_evicts_least_recently_used`.

---

### Task 16: The suffix strip, untraced answers, the Bearer arm

**Spec:** 4.2 (the strip, D26), 4.6 (use, D7, D23, D27), 4.8 (D9), 8 (D24 and the traces), invariants 61, 65, 72, 73 and 74, and the two exceptions of section 10.

**Files:**
- Modify `crates/common/src/za/tokens.rs`: `za_split_bearer` and its unit test.
- Modify `crates/common/src/auth/oauth/token.rs:162-170` (`decode_access_token`) and `crates/common/src/auth/oauth/token_restored_tests.rs`.
- Modify `crates/http/src/auth/oauth/token.rs:652-668` (`token_response` only), with a test module at the file's end.
- Create `crates/http/src/auth/za_bearer.rs`; modify `crates/http/src/auth/mod.rs:7-9` (`pub(crate) mod za_bearer;`).
- Modify `crates/http/src/auth/authenticate.rs:195-277`.
- Modify `tests/src/utils/oauth.rs` (the Bearer helpers).
- Test: `tests/src/za/tokens.rs`.

**Interfaces:**
- Consumes: `split_secret`, `ZaSuffix`, `append_secret` (I §5.3); `za_token_open_access`, `za_token_issue` (I §5.4). Existing: `TOKEN_HEADER` (`crates/common/src/auth/oauth/token.rs:23`), `AccessToken::grant_id` (`crates/common/src/auth/access_token.rs:576-578`), `Server::account` (it fails closed), `AccountCache::is_key_account` and `za_generation` (`crates/common/src/auth/mod.rs:474-485`), `HttpResponse::with_untraced_body` (`crates/http-proto/src/response.rs:299`), the http pause `za_test::pause_point` (`authenticate.rs:209-210`).
- Produces: `za_split_bearer` (I §5.4); `za_bearer_keys` and the rows for `common/src/auth/oauth/token.rs`, `token_response` and `authenticate.rs` (I §5.5); `za_split_token`, `za_tamper_secret`, `CALENDAR_JMAP_USING`, `za_bearer_jmap`, `za_bearer_session`, `za_bearer_dav` (I §10).

**Constraints:**
- `za_split_bearer` is the one splitter of a Bearer value: `decode_access_token`, `za_bearer_keys` and, from Task 17, `refresh_token_grant` call it, and none calls `split_secret` itself. A value that does not begin with the server's token header is never split, whatever characters it holds (spec 4.2).
- `decode_access_token` reads no secret and attaches nothing to an error (upgrade ruling 171 stays).
- An error answer of the token endpoint stays traced; only a granted one is untraced (invariant 73).
- A token that carries a secret is served with keys or refused with 401; it is never served keyless (invariant 65, D7).
- Keys opened from a token are served only at the account's current generation; on a mismatch the request is refused, never served uncached (invariant 72, D23). A password login in that state is still served uncached, as today.
- Only the HTTP layer reads the token record. `authenticate_with_keys` and `crates/common/src/auth/authentication.rs` are not touched, so the IMAP and POP3 logins get no keys from a token (D9). The Basic arm is not touched.
- The key cache and `http_auth` stay keyed on the token as presented, suffix included (invariant 61).
- A refusal is an `AuthEvent::Failed` error with one of the details below (the texts are I §5.6's), the account id and, where known, the grant id. A refusal of the fork (an entry that is missing or does not open, a malformed suffix, a stale generation) is not counted as an authentication failure for banning: no `authentication_failure` call, no delay.

  | Condition | Details |
  |---|---|
  | `ZaSuffix::Malformed` | `zero-access: malformed token secret` |
  | a suffix, and the account is not a key account | `zero-access: token secret on an ordinary account's token` |
  | a suffix, and the token has no grant id | `zero-access: token secret on a token without a grant` |
  | `za_token_open_access` returned `None` | `zero-access: token keys unavailable` |
  | the generation fence fails for token keys | `zero-access: token keys of a superseded generation` |

- [ ] **Step 1: write the failing unit tests.**

  `a_secret_suffix_is_outside_the_token_alphabet`, in `crates/common/src/auth/oauth/token_restored_tests.rs`, on `seal_token(KEY, &sample(GrantType::AccessToken, None, 3), NAME)`:
  - The token holds no `~` (spec 4.2 relies on it).
  - With a secret appended (`append_secret`), `open_token` fails.
  - `open_token(KEY, crate::za::tokens::za_split_bearer(&keyed).0)` has the sample's fields.

  `a_value_without_the_token_header_is_not_split`, in `crates/common/src/za/tokens.rs`:
  - `za_split_bearer("opaque~value")` is `("opaque~value", None)`.
  - `"sw1.AAAA.BBBB"` with a secret appended splits into `("sw1.AAAA.BBBB", Secret)` with the secret's bytes.

  `granted_token_answer_is_untraced`, in a new `#[cfg(test)] mod za_tests` of `crates/http/src/auth/oauth/token.rs`:
  - `token_response(TokenResponse::Granted(..), false).is_untraced()`.
  - `token_response(TokenResponse::error(ErrorType::InvalidGrant), false)` and `(.. ErrorType::InvalidClient .., true)` are not.

- [ ] **Step 2: run them.** U-common with `a_secret_suffix_is_outside_the_token_alphabet`, then with `a_value_without_the_token_header_is_not_split`. Expected: the crate does not compile; `za_split_bearer` is not found. U-http with `granted_token_answer_is_untraced`. Expected: FAILED at its first assertion.

- [ ] **Step 3: `za_split_bearer` and the strip.** `za_split_bearer` is `split_secret(token, TOKEN_HEADER)`. In `decode_access_token` (`crates/common/src/auth/oauth/token.rs:170`), pass `za_split_bearer(token_).0` to `open_token`. Extend the comment at `:167-169` with spec 4.2: every consumer accepts the longer string.

- [ ] **Step 4: the untraced answer.** In `token_response`, mark the response `with_untraced_body()` when `response` is `TokenResponse::Granted`, for every account, with a comment citing D24.

- [ ] **Step 5: run the unit tests again.** The three names of step 2. Expected: all pass.

- [ ] **Step 6: write the Bearer helpers of I §10** in `tests/src/utils/oauth.rs`. `za_bearer_jmap` posts `{"using": using, "methodCalls": calls}` to `{SERVER_URL}/jmap` with a five-second client and returns the status and the body (`JmapResponse(Value::Null)` when the body is not JSON). `za_bearer_session` is a `GET {SERVER_URL}/jmap/session`. `za_bearer_dav` is a `DummyWebDavClient` with `credentials = format!("Bearer {token}")` (`tests/src/za/dav_gate.rs:112-114`). `za_tamper_secret` swaps the first character after `~` between `A` and `B`, so the text stays a canonical secret.

- [ ] **Step 7: write the failing za cases.** A "keyed token" is Task 15's `minted`, issued, with `append_secret(&mut token, &issued.access)`. `CAL` is `/dav/cal/token-owner%40example.com/`. `MAIL_GET` is `[["Email/get", {"accountId": .., "ids": []}, "g"]]`.

  `keyed_bearer_reads_sealed_data`. An event with `SUMMARY:token canary event` is written over Basic CalDAV (`uid_lease::put`, 201) and a message delivered with `mail::deliver(test, "sender@example.org", OWNER, &mail::message_with("token canary subject", "body"))`:
  - `za_bearer_dav(&owner, &keyed)` `GET` of the event answers 200 and the body holds `SUMMARY:token canary event`.
  - `za_bearer_jmap(&keyed, CALENDAR_JMAP_USING, ..)` with `Email/query` and an `Email/get` of its ids (`properties: ["subject"]`) answers 200 and the first subject is `token canary subject`.
  - `za_bearer_session(&keyed)` answers 200, and `test.server.inner.cache.za_keys.contains_account(id)`.

  `tampered_or_foreign_secret_is_401`, after a served request has cached the good token; `upstream` is `za_split_token(&keyed).0` and `other_issued` the issue of a second minted grant:

  | Token | Details traced |
  |---|---|
  | `za_tamper_secret(&keyed)` | `zero-access: token keys unavailable` |
  | `upstream` with the grant's own refresh secret appended | the same |
  | `upstream` with `other_issued.access` appended | the same |
  | `{upstream}~short` | `zero-access: malformed token secret` |
  | `{keyed}~` | the same |

  - For each: `za_bearer_session` answers 401; `za_bearer_dav` `PROPFIND` of `CAL` answers 401; `za_bearer_jmap` with `MAIL_GET` answers 401 (never served keyless, D7).
  - A `Traced` over `Auth(AuthEvent::Failed)` holds an event with each of the two details and the account id; no event's text holds the secret or `upstream`.
  - `za_bearer_session(&keyed)` then answers 200. A comment there cites spec 4.6 (D27): fifteen refusals from one address are not counted for banning, so these requests carry no `X-Forwarded-For`.

  `suffix_on_ordinary_token_is_401`, with `za_grant_token` of `PLAIN`:
  - The token answers 200 at `za_bearer_session` (control).
  - With `append_secret` of a random secret it answers 401, traced with `zero-access: token secret on an ordinary account's token`.
  - The token without the suffix answers 200 again. The grant is revoked.

  `keyed_token_without_secret_is_keyless`, with `upstream` alone:
  - `za_bearer_session` answers 200.
  - `za_bearer_jmap` with `MAIL_GET` answers 200 with the method response `error`, type `forbidden`, description `common::za::ZA_MAIL_KEYLESS`.
  - `za_bearer_dav` `PROPFIND` of `CAL` answers 403.

  `bearer_crossed_by_vault_write_is_refused`. Hook: the http pause `za_test::set`, which now sits between `za_bearer_keys` and the generation fence. A `Traced` over `Auth(AuthEvent::Failed)` spans steps 2 to 4.
  1. A keyed token is minted and issued; no request has used it.
  2. `Parked::start(id, za_test::set, za_bearer_session(&keyed))`: the request has opened MK at revision r and waits.
  3. `za_post("password", {username: OWNER, password: STRONG, new_password: STRONG2})` answers 200: the vault is at r+1.
  4. `parked.finish()`.
  5. The status is 401 (D23: refused, not served uncached); `assert_nothing_cached(test, id)`; an event carries `zero-access: token keys of a superseded generation`.
  6. Control: `park_login(id, OWNER, STRONG2)` crossed by a password change back to `STRONG` answers 207. The account ends at `STRONG`.
  7. The same with a vault write that changes no credential: a second keyed token parked at the http pause and crossed by `za_post("recovery-key", ..)`, 200. The parked request answers 401 with the same event (D23 compares generations). The same token's next request answers 200: its keys are opened again from the token record at the new revision.

  `mail_bearer_login_stays_refused`, as `tests/src/za/mail/login.rs:215-258` with the keyed token in the `OAUTHBEARER` and `XOAUTH2` responses:
  - IMAP, both mechanisms: `k1 NO [NOPERM] {ZA_MAIL_LOGIN_REFUSED}`.
  - POP3 `XOAUTH2`: `-ERR {ZA_MAIL_LOGIN_REFUSED}`.
  - `live_keys(id)` equals its value before, after each (D9).

  `revocation_and_introspection_accept_the_suffix`:
  - `test.server.decode_access_token(None, &keyed)` succeeds with the account id and the grant id (the consumer `/auth/revoke` uses).
  - `introspect_access_token(&keyed)` is `active`, and so is `introspect_access_token(&za_tamper_secret(&keyed))` (4.2: upstream's state only).

- [ ] **Step 8: run them.** ZA. Expected: FAILED in `keyed_bearer_reads_sealed_data` at the CalDAV `GET`: 403 where 200 is expected, since a Bearer token carries no keys yet.

- [ ] **Step 9: implement `za_bearer_keys`** (I §5.5): `za_split_bearer(token).1`; `None` returns `Ok(None)`; then the refusals of the table, in its order (`server.account(access_token.account_id())` for the account class, `access_token.grant_id()`), then `server.za_token_open_access(account_id, grant_id, &secret)`.

- [ ] **Step 10: the Bearer arm** in `authenticate_uncached`:
  - After `authenticate_with_keys` returns (`:207`) and before the pause point (`:209-210`): when `mechanism` is `bearer`, call `za_bearer_keys(server, token, &access_token)`; its keys become the request's `session_keys`, and a flag records that they came from a token.
  - In the keyed arm (`:262-277`): when the generation fence fails and the keys came from a token, return the `AuthEvent::Failed` error of the table's last row instead of serving the request. Keys of a password login keep today's behaviour.
  - Update the function's comment (`:153-155`) and the arm's (`:263-270`): keys are still attached here only; the Bearer arm is a second source beside Basic (release 1 invariant 5).

- [ ] **Step 11: run the suites.** ZA: the `tokens` module prints the seven case names, and `dav_gate`, `caches`, `mail::login` and `tracing` are unchanged. SYS: `tests/src/system/oidc.rs` runs upstream's flows through the strip and the untraced answer.

- [ ] **Step 12: the closing checks.**

- [ ] **Step 13: commit.** `Release 6: Bearer tokens that carry keys on HTTP`

Catchers for Task 42: "a token with an unopenable secret is served keyless" is `tampered_or_foreign_secret_is_401`; "a Bearer request at a stale generation is served" is `bearer_crossed_by_vault_write_is_refused`; "a successful token answer is traced" is `granted_token_answer_is_untraced` (U-http) and Task 17's `granted_answer_is_untraced` (ZA).

---

### Task 17: The hand-off, the code exchange, the refresh precheck and its interim refusal

**Spec:** 4.4 (D6, the three paths, the rules, D27), 4.5 (D22), 4.8 (the device flow), 4.9, 9.2 (the paused first issuance), invariants 61, 62, 64 and 71.

**Files:**
- Modify `crates/common/src/za/tokens.rs`: the three hand-off functions and the precheck.
- Create `crates/http/src/auth/oauth/za_token.rs`; modify `crates/http/src/auth/oauth/mod.rs:13-18` (`mod za_token;`).
- Modify `crates/http/src/auth/oauth/auth.rs`: `LoginAccount` (`:600-609`), `authenticate_login` (`:611-686`), the `ConsentHandler` trait (`:712-740`), `authorize_code` (`:773-844`), `request_consent` (`:846-879`), `confirm_consent` (`:881-915`), `issue_auth_code` (`:949-985`), `approve_device` (`:1052-1058`).
- Modify `crates/http/src/auth/oauth/token.rs`: `authorization_code_grant` (`:272-337`), `device_code_grant` (`:380-390`), `refresh_token_grant` (before `:484`), `issue_new_grant` (`:510-582`), `NewGrantRequest` (`:627-636`).
- Modify `tests/src/utils/oauth.rs` (the sign-in helpers); `tests/src/za/app_password.rs:686-707` (one comment); `tests/src/za/sweep.rs:46-53` (it calls the lifted `run_purge_maintenance`).
- Test: `tests/src/za/tokens.rs`.

**Interfaces:**
- Consumes: `seal_handoff`, `open_handoff`, `split_code`, `append_code_secret`, `append_secret`, `ZaSuffix` (I §5.3); `za_session_binding` (I §5.1); `za_token_issue`, `ZaTokenIssue`, `za_split_bearer`, `token_read_store` (I §5.4); `TOKEN_EXCHANGE_PAUSE` (I §9). Existing: `Server::authenticate_with_keys` (`crates/common/src/auth/authentication.rs:73-119`), `KV_OAUTH`, `KeyValue::with_prefix(..).expires(secs)`, `key_set`, `key_get`, `key_delete`, `key_exists` (`crates/store/src/dispatch/lookup.rs`), `Server::za_vault_record`, `delete_oauth_grant`.
- Produces: `ZaHandoff`, `za_handoff_put`, `za_handoff_take`, `za_handoff_delete`, `ZaRefreshPrecheck`, `za_token_refresh_precheck` (I §5.4); `ZaGrantKeys`, `za_code_handoff`, `za_issue_keyed` and the rows for `auth.rs` and for `authorization_code_grant`, `issue_new_grant` and the precheck call of `refresh_token_grant` (I §5.5); the sign-in helpers of I §10 (`ZaClientKind`, `ZaOAuthClient`, `za_oauth_client`, `za_drop_oauth_client`, `ZA_SIGNIN_SCOPE`, `ZaTokens`, `za_authorize`, `za_confirm_consent`, `za_exchange`, `za_refresh`, `za_revoke_token`, `za_sign_in`, `handoff_exists`, `run_purge_maintenance`, `OAuthExpiryGuard`, `oauth_expiry`).
- Existing signatures this task changes: `LoginAccount` gains `za_keys: Option<Arc<SessionKeys>>`; `issue_auth_code` and `request_consent` gain `za_keys: Option<&SessionKeys>`; `NewGrantRequest` gains `za_grant: Option<ZaGrantKeys>`.
- Beyond the sheet, private to `tests/src/za/tokens.rs`: `code_of(location)`, the location's `code` parameter parsed with `reqwest::Url`; `grants_of(test, account_id)`, the registry query of the account's `OAuthGrant` ids (`tests/src/za/app_password.rs:662-668`).

**Constraints:**
- For a key account the authorization-code flow yields a keyed grant or none (spec 4.4). A code exchange with no hand-off, one that does not open, or one made under a binding that is no longer current answers `invalid_grant`.
- The hand-off entry is written before the code or the consent token it belongs to is stored. It carries the code's expiry, `oauth_expiry_auth_code`.
- The hand-off secret is appended to the code or the consent token with no separator and is stored nowhere (invariant 61). MK leaves process memory only inside the hand-off's wrap (invariant 62).
- The entry is deleted when the code is redeemed, when a consent is refused, and when a confirmation fails after its consent token was taken. No hand-off outlives its code.
- `za_handoff_put` at a stale revision returns `AuthEvent::Failed` with its text of I §5.6. The password step then answers the authentication error without `authentication_failure`: the password was right.
- A lost vault assertion is retried under an unchanged binding, with at most three attempts in all (Task 15, spec 4.3). A changed binding answers `invalid_grant`; exhausted retries and other failures answer a server error. If issuance fails after the grant was created, the grant is deleted and no wrap is retained (spec 4.4).
- The device flow calls the same login function and drops the keys (D9). `issue_new_grant` decides by the hand-off it was passed, never by the account.
- `OAuthCode` is a stored rkyv struct: its layout does not change. Upstream's checks of the client, the redirect URI, PKCE and single use run unchanged and first, on the code's 40 characters. `take_pending_consent` accepts exactly 40 characters.
- An ordinary account's sign-in writes no hand-off and gets no suffix. It pays one cached account read at a code exchange and at a refresh, and one in-memory delete at a consent confirmation that carries no secret. A secret on its code answers `invalid_grant`.
- A key account's refresh reads the token record before the grant rotates; a refresh token without a secret, or with a suffix that is not one, never rotates a grant that has entries (invariant 71, D22).
- `OAuthResponse` derives `Debug` and now holds secrets: it is never traced.
- The in-memory store returns only types that implement `Deserialize`, `From<store::Value<'static>>` and `Debug`. The hand-off value is read into a private newtype over `Vec<u8>` written as `RawValue` is (`crates/common/src/auth/vault.rs:46-52`), whose `Debug` prints no byte.

- [ ] **Step 1: write the sign-in helpers of I §10** in `tests/src/utils/oauth.rs`.
  - `za_oauth_client`: `Registered` creates a registry `OAuthClient` with the redirect `https://example.org/{client_id}/` and the grant types of I §10; `Application` adds an enabled `Application` object naming it and reloads the applications (`tests/src/za/app_password.rs:546-577`); `Stateless` registers through `POST /auth/register` (`tests/src/system/oidc.rs:292-448`). `za_drop_oauth_client` destroys them and reloads (`app_password.rs:744-756`).
  - `za_authorize` posts the `authCode` form of `app_password.rs:613-626` to `/api/auth`, with the client's id and redirect, the scope `ZA_SIGNIN_SCOPE`, the S256 challenge `E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM`, `mfaToken` and `consent` when given, and `X-Forwarded-For` when `from_ip` is given. `za_confirm_consent` posts `authConsent` with the same query string.
  - `za_exchange`, `za_refresh` and `za_revoke_token` post forms to `/auth/token` and `/auth/revoke`; the verifier is `dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk`.
  - `za_sign_in` is `za_authorize` with `consent: Some(true)`, the code parsed from the location with `reqwest::Url`, then `za_exchange`; it panics unless the answer is `Granted`. `grant_id` comes from `test.server.decode_access_token(None, &access)`.
  - `handoff_exists` is `in_memory_store().key_exists(KeyValue::<()>::build_key(KV_OAUTH, "~" + the first 40 characters))`.
  - `oauth_expiry` swaps `oauth_expiry_token`, `oauth_expiry_refresh_token` and `oauth_expiry_auth_code` on the running core (`tests/src/system/oidc.rs:960-967`) and restores the previous values on drop.
  - `run_purge_maintenance` is `tests/src/za/sweep.rs:46-53` lifted; `sweep.rs` calls it in its place.

- [ ] **Step 2: write the failing za cases.** Add the key accounts `token-consent` (`CONSENT`) and `token-race` (`RACE`), and the clients `app` (`za-token-app`, `ZaClientKind::Application`) and `registered` (`za-token-consent`, `ZaClientKind::Registered`), each held in an `Arc` so a parked request owns it. The module's end drops both with `za_drop_oauth_client`. Every case revokes the grants it made, so `grants_of` is empty between cases.

  `sign_in_gives_keyed_tokens`, `tokens = za_sign_in(test, &app, OWNER, STRONG, None)`:
  - The access token's secret (`za_split_token(..).1`) is 43 characters, and the token is its upstream part plus 44 characters (4.9).
  - The refresh token's secret is 43 characters and differs from the access token's.
  - Sealed data is read over JMAP and CalDAV, as Task 16's `keyed_bearer_reads_sealed_data` asserts it.
  - The record's grant holds one refresh entry of generation 0 and one access entry.
  - `za_revoke_token(&app, &tokens.refresh)` answers 200; `oauth_grant` is `None`; `raw_token_record` is `None`; the access token answers 401.

  `sign_in_with_second_factor`, on `MFA` with a second factor enrolled for the case and removed after it:
  - `za_authorize(&app, MFA, STRONG, None, Some(true), None)` is `LoginResponse::MfaRequired`.
  - `za_sign_in(.., Some(&code(&url)))` gives an access token with a secret, which answers 200 at `za_bearer_session`.

  `consent_path`, on `CONSENT` with `registered`:
  1. `za_authorize(&registered, CONSENT, STRONG, None, None, None)` is `ConsentRequired { token }`; `token.len() == 83`; `handoff_exists(test, &token)`.
  2. `za_confirm_consent(&registered, &token, true)` is `Authenticated`; the token's hand-off is gone; the code is 83 characters and its hand-off exists.
  3. `za_exchange` of the code is `Granted` with a secret on the access token; the code's hand-off is gone.
  4. A grant now exists for the client and scopes: `za_authorize` with `consent: None` is `Authenticated` at once, and its code exchanges to a keyed pair.
  5. Both grants are revoked (`za_revoke_token`, 200 each), so consent is asked again.
  6. Refused: `ConsentRequired { token }`, then `za_confirm_consent(.., &token, false)` is `Redirect`, and the hand-off is gone.
  7. Confirmed without the secret: `ConsentRequired { token }`, then `za_confirm_consent(.., &token[..40], true)` is `Authenticated`; the token's hand-off is gone; the code is 40 characters; its exchange answers `invalid_grant`.
  8. `grants_of` is empty and `raw_token_record` is `None`.

  `exchange_without_handoff_is_invalid_grant`, on `OWNER` with `app`:
  - A code from `za_authorize(.., Some(true), None)`: its hand-off exists. `za_exchange(&app, &code[..40])` answers `invalid_grant`; the hand-off is gone; the exchange of the full code answers `invalid_grant` too (the code is spent).
  - A second code, presented as its 40 characters with another secret appended (`append_code_secret` of `Secret::random()`): the exchange answers `invalid_grant` and the hand-off is gone.
  - `grants_of` is empty and `raw_token_record` is `None`.

  `ordinary_code_with_suffix_is_invalid_grant`, on `PLAIN` with `app`:
  - `za_authorize` gives a code of 40 characters.
  - With 43 valid characters appended (`append_code_secret(.., &Secret::random())`) the exchange answers `invalid_grant`.
  - `grants_of(test, plain_id)` is empty; the exchange of the 40 characters alone answers `invalid_grant` (the code is spent).

  `handoff_gone_after_redemption_and_expiry`:
  - A code's hand-off exists; its exchange is `Granted`; the hand-off is gone (deleted on redemption).
  - Under `oauth_expiry(test, None, None, Some(2))`: a code, a sleep of three seconds, the hand-off is gone and the exchange answers `invalid_grant`.

  `device_flow_stays_keyless`, with `registered` and the device forms of `tests/src/za/app_password.rs:630-717`:
  - `authDevice` with `STRONG` answers `{"type": "verified"}`.
  - Neither the access token nor the refresh token holds `~`.
  - `raw_token_record` is `None`, and `Email/get` with the access token answers the keyless refusal of Task 16's `keyed_token_without_secret_is_keyless`.

  `refresh_without_secret_of_keyed_grant_refused`, after a sign-in `t0` of `OWNER`; `upstream` is the refresh token's upstream part:
  - `before` is `oauth_grant(grant)` and `raw` is `raw_token_record`.
  - `za_refresh(&app, upstream)` and `za_refresh(&app, "{upstream}~short")` each answer `invalid_grant`; the grant's `generation` and `pending` equal `before`'s (D22: the grant does not rotate); `raw_token_record` equals `raw`.
  - **Interim, deleted by Task 18:** `za_refresh(&app, &t0.refresh)`, with its secret, also answers `invalid_grant` and leaves the generation, the pending flag and the record's bytes unchanged.
  - Control: the refresh token of a device-flow grant (keyless, as in `device_flow_stays_keyless`) is `Granted` through `registered`, and neither token of the answer holds `~`.

  `exchange_crossed_by_password_change`, on `RACE`. Hook: `TOKEN_EXCHANGE_PAUSE`.
  1. `za_authorize(&app, RACE, STRONG, None, Some(true), None)`; the code is taken.
  2. `HookParked::start(race, test_hooks::set_token_exchange_pause, za_exchange(&app, &code))`. The exchange waits in `za_token_issue`: the grant exists, the record does not.
  3. While it is parked: `raw_token_record` is `None` and `grants_of` holds one grant.
  4. `za_post("password", {username: RACE, password: STRONG, new_password: STRONG2})` answers 200.
  5. `parked.finish()`.
  6. The answer is `invalid_grant`; `raw_token_record` is `None` (no record); `grants_of` is empty (no grant); the code's hand-off is gone.
  7. A sign-in with `STRONG2` then succeeds and reads sealed data.

  `exchange_crossed_by_recovery`: the same steps, signing in with `STRONG2` and with step 4 replaced by `za_post("recover", {username: RACE, recovery_key, new_password: STRONG})`. The same assertions. The account ends at `STRONG`.

  `exchange_crossed_by_other_vault_write`, on `RACE`. Hook: `TOKEN_EXCHANGE_PAUSE`. Steps 1 to 3 as above. Step 4 is `za_post("recovery-key", {username: RACE, password: STRONG})`, 200: a vault write that changes no credential. Then `parked.finish()`: the answer is `Granted` with keyed tokens; the record holds the grant's first entries under the binding of the sign-in; the access token reads sealed data; `grants_of` holds one grant. The exchange lost its vault assertion once and wrote on its second attempt (spec 4.3). The grant is revoked.

  `thirty_third_sign_in_ends_the_oldest`, on `CAP`:
  1. 32 sign-ins, `tokens[0]` first.
  2. A sleep of 1,100 ms; then the first grant is used, in process as in Task 15's cap case: `rotate_oauth_grant(.., 0)` is `Issue { .. }`.
  3. `last = za_sign_in(test, &app, CAP, STRONG, None)`.
  4. The record holds 32 grants, lacks `tokens[1].grant_id` (D20) and holds `tokens[0].grant_id`; `oauth_grant(tokens[1].grant_id)` is `None`.
  5. `tokens[1].access` answers 401 and `za_refresh` of `tokens[1].refresh` answers `invalid_grant`; `last.access` and `tokens[0].access` answer 200.
  6. The 32 grants that remain are revoked.

  `granted_answer_is_untraced`, under `Traced::start(EventType::variants())`:
  - A sign-in of `OWNER` and one of `PLAIN`, both through `app`; the ordinary account's tokens carry no secret.
  - `za_exchange(&app, "x" 40 times)` answers `invalid_grant`.
  - No event's text holds the upstream part or the secret of any of the four tokens (D24).
  - Among the `Http(HttpEvent::ResponseBody)` events one holds `[redacted]` and one holds `invalid_grant` (the control: an error answer is still traced).

- [ ] **Step 3: run them.** ZA. Expected: FAILED in `sign_in_gives_keyed_tokens` at its first assertion: the access token has no secret.

- [ ] **Step 4: implement the hand-off functions** (I §5.4). The key is `KeyValue::<()>::build_key(KV_OAUTH, [b"~", key_id.as_bytes()].concat())`.
  - `za_handoff_put`: read the vault record; a missing record, `keys.account_id != account_id` or `record.revision != keys.generation` is the `AuthEvent::Failed` error of the constraints; `seal_handoff(keys.mk(), account_id, za_session_binding(&record), key_id)`; `key_set` with `expires`; return the secret.
  - `za_handoff_take`: `key_get`; when an entry was found, `key_delete` whatever follows; `open_handoff(&value, secret, key_id)`.
  - `za_handoff_delete`: `key_delete`, its result ignored.

- [ ] **Step 5: the password step** in `crates/http/src/auth/oauth/auth.rs`.
  - `authenticate_login` calls `authenticate_with_keys` and puts the keys in `LoginAccount.za_keys`. Every refusal after the login (`:636-663`) drops them with the function's return.
  - `issue_auth_code`: with `za_keys`, call `za_handoff_put(&client_code, account_id, keys, self.core.oauth.oauth_expiry_auth_code)` before the code is stored (`:977-982`), and build the location from the code with the secret appended (`append_code_secret`). The store key stays the 40 characters.
  - `request_consent`: the same under the consent token (`:852-876`); the returned `ConsentRequired.token` carries the secret.
  - `authorize_code` passes `login.za_keys.as_deref()` to both (`:833`, `:837-843`). The refused-consent and tenant returns write nothing.
  - `confirm_consent`, in this order:
    1. `split_code(token)`; its 40 characters go to `take_pending_consent`. A token that is not taken returns as today and deletes nothing.
    2. Once the token was taken: with `ZaSuffix::Secret`, `za_handoff_take`; otherwise `za_handoff_delete`. A hand-off of another account counts as missing.
    3. For a taken hand-off, read the vault record (`za_vault_record`), require `za_session_binding(&read.record) == handoff.binding`, and build `SessionKeys::new(account_id, read.record.revision, handoff.mk)`. A missing record or a mismatch counts as a missing hand-off.
    4. Pass the keys, or `None`, to `issue_auth_code`. Both returns after the token was taken (`:903-904`, `:910-913`) leave no entry: step 2 removed it.
  - `approve_device` takes no keys from `login`; a comment cites D9.

- [ ] **Step 6: implement `za_code_handoff` and `za_issue_keyed`** (I §5.5).
  - `za_code_handoff`, by `server.account(account_id)` (it fails closed):

    | Account | Suffix | Answer |
    |---|---|---|
    | ordinary | `None` | `Ok(Ok(None))` |
    | ordinary | `Secret` | `Ok(Err(ErrorType::InvalidGrant))`; the code is spent, as upstream's checks consumed it |
    | key | `Secret` | `za_handoff_take(code, &secret)`; `Some` of this account gives `ZaGrantKeys { mk, binding }`; otherwise `Ok(Err(InvalidGrant))` |
    | key | `None` | `za_handoff_delete(code)`, then `Ok(Err(InvalidGrant))` |

  - `za_issue_keyed`: the expiries are `now() + server.core.oauth.oauth_expiry_token` and `now() + server.core.oauth.oauth_expiry_refresh_token`. `Issued`: `append_secret` to `response.access_token` and to `response.refresh_token` when it is `Some`. `VaultChanged`: delete the grant (`oauth_grant`, `delete_oauth_grant` with `GrantDeletion::Unconditional`, `invalidate_caches`) and answer `Ok(Err(ErrorType::InvalidGrant))`. An error: delete the grant the same way, logging a failure of the deletion, and return the error.

- [ ] **Step 7: the code exchange** in `crates/http/src/auth/oauth/token.rs`.
  - `authorization_code_grant`: `split_code` on the `code` parameter (`:279-285`); the 40 characters go to `key_get` (`:287-293`) and `consume_oauth_code` (`:321`). After `:324` call `za_code_handoff`; an `Err(error)` returns `TokenResponse::error(error)`. Pass the result as `NewGrantRequest.za_grant`.
  - `device_code_grant` passes `za_grant: None`.
  - `issue_new_grant`: after `grant_response` (`:564-575`), with `request.za_grant`, call `za_issue_keyed`; an `Err(error)` returns `TokenResponse::error(error)`.

- [ ] **Step 8: the refresh precheck and its interim refusal.**
  - `za_token_refresh_precheck` (I §5.4) reads field 153 in every case (invariant 71). The grant "has entries" when a usable record names it. `Malformed`: `Refused`. `None`: `Refused` when the grant has entries, else `Keyless`. `Secret`: `Keyed`.
  - In `refresh_token_grant`, immediately before `rotate_oauth_grant` (`:484`) and after every upstream check: when `self.account(token_info.account_id)` (it fails closed) is a key account, call `za_token_refresh_precheck(token_info.account_id, presented.id, &za_split_bearer(refresh_token).1)`. `Refused` returns `TokenResponse::error(ErrorType::InvalidGrant)`. `Keyless`, and every ordinary account, goes on as upstream does.
  - `Keyed` also returns `invalid_grant`, in this task only, with the comment "Task 18 replaces this arm with the keyed refresh". A keyed grant therefore never refreshes into a keyless pair, at any commit.

- [ ] **Step 9: the comment in `tests/src/za/app_password.rs:686-707`.** State that from release 6 this exchange yields a keyed grant and a token record (spec 9.6), that the `revoke` at the case's end is a vault write that changes no credential, so the record stays until its grant or the account goes (spec D19), and that the emptiness check exempts field 153 while the account exists. No assertion changes.

- [ ] **Step 10: run the suites.** ZA: the `tokens` module prints the thirteen case names; `app_password` (`oauth_sign_in_refuses_vault_app_password`), `mail::gates` (`tests/src/za/mail/gates.rs:1985-2100`), `tracing` and `sweep` pass through the changed functions. SYS.

- [ ] **Step 11: the closing checks.**

- [ ] **Step 12: commit.** `Release 6: the sign-in hand-off and keyed grants at the code exchange`

Catchers for Task 42: "the hand-off is not deleted on redemption" is `handoff_gone_after_redemption_and_expiry`, its only catcher: upstream's single-use code already refuses a replayed exchange. "A token-record write omits the vault assertion" is `exchange_crossed_by_password_change`. "A refresh without the secret of a keyed grant rotates it" is `refresh_without_secret_of_keyed_grant_refused`.

---

### Task 18: Refresh

**Spec:** 4.5 (the six steps, the superseded case), 9.2 (the paused refreshes), invariants 64 and 67.

**Files:**
- Modify `crates/common/src/za/tokens.rs`: `za_token_refresh`.
- Modify `crates/common/src/za/test_hooks.rs`: three hooks, in the places Task 15 used.
- Modify `crates/http/src/auth/oauth/za_token.rs`: `za_refresh_keyed`.
- Modify `crates/http/src/auth/oauth/token.rs:406-508` (`refresh_token_grant`: Task 17's `Keyed` arm and the `Issue` arm).
- Test: `tests/src/za/tokens.rs`.

**Interfaces:**
- Consumes: `classify_refresh`, `ZaRefreshClass`, `put_refresh`, `put_access`, `reconcile` (I §5.3); `token_read_store`, `token_write_store`, `ZaIssued`, `ZaRefreshPrecheck` (I §5.4); the sign-in helpers (I §10). Existing: `Server::rotate_oauth_grant` (`crates/common/src/auth/oauth/grant.rs:421-507`), `TokenGrant`.
- Produces: `ZaTokenRefresh`, `za_token_refresh` (I §5.4); `za_refresh_keyed` and the rest of the row for `refresh_token_grant` (I §5.5); `TOKEN_REFRESH_READ_PAUSE`, `TOKEN_REFRESH_WRITE_PAUSE`, `TOKEN_REFRESH_WRITE_FAIL_ONCE` with their setters and points (I §9).

**Constraints:**
- Upstream's state machine is followed, not changed: `rotate_oauth_grant` and `StoredOAuthGrant::rotation` are not touched. Upstream accepts two generations: the stored one, whose presentation while a rotation is pending is a retry and returns the same next token, and the one after it, whose presentation rotates. Anything older revokes the grant.
- A keyed grant never becomes a keyless one: when the keys are lost the grant is deleted.
- A refresh whose generation was superseded while it was in flight answers `invalid_grant` and changes nothing: no grant is deleted and no entry is written.
- The write carries both assertions. After an assertion loss the vault record and the token record are read again; a changed binding stops the refresh. Otherwise it classifies again from the record as it now is, three attempts in all. MK held from an earlier read is never used to put back an entry the record no longer holds: every attempt takes MK from that attempt's own record.
- No write adds a refresh entry below its grant's stored generation (invariant 67).
- A retry returns the same refresh secret: it is opened from the presented entry's sealed next secret, never drawn again.

- [ ] **Step 1: write the failing za cases.** `g(n)` is the refresh token of generation `n`; a sign-in gives `g(0)`. `generations(test, id, grant)` reads the grant's refresh generations from `token_record`. Each case starts from a new sign-in through `app` and revokes its grant when it ends.

  In Task 17's `refresh_without_secret_of_keyed_grant_refused`, delete the interim assertion and put in its place: `za_refresh(&app, &t0.refresh)` is `Granted`.

  `refresh_rotates_and_reads`:
  - `za_refresh(&app, &g0)` is `Granted(t1)`; its refresh token differs from `g(0)`; its access token carries a secret and answers 200.
  - The sign-in's access token still answers 200 (it lives until it expires).
  - `generations` is `[0, 1]`; after `za_refresh` of `g(1)`, `[1, 2]` (the presented generation and the next one).

  `retried_refresh_returns_the_same_token`:
  - `za_refresh(&app, &g0)` twice: both `Granted`, with the same refresh token string, secret included, and different access tokens.
  - Both access tokens answer 200; the record's grant holds three access entries.

  `replayed_refresh_revokes`, after `g(0)`, `g(1)`, `g(2)` (the stored generation is 1):
  - `za_refresh(&app, &g0)` answers `invalid_grant`; `oauth_grant` is `None` (upstream revokes on a replay); `raw_token_record` is `None`.
  - `za_refresh(&app, &g2)` answers `invalid_grant`.

  `superseded_token_opens_nothing`, after the same three; `g(0)` is not presented, since presenting it would revoke the grant:
  - No refresh entry has generation 0.
  - `record.classify_refresh(id, grant, 0, &secret0, now())` is not `Opened`.
  - Generation 1's sealed `next` does not open under `g(0)`'s secret: `unwrap_key(next, &derive_token_next_key(&secret0), &token_next_aad(id, grant, 1))` is an error.

  `refresh_with_missing_entry_deletes_grant`:
  - `za_token_forget_grant(id, grant)`: the entries leave and `oauth_grant` is still `Some`.
  - `za_refresh(&app, &g0)` answers `invalid_grant` and `oauth_grant` is `None` (4.5 step 2: the keys are lost, the grant ends).

  `refresh_crossed_by_credential_change`, on `RACE`, three runs. Each run:
  1. Sign in with the account's password now: `g(0)` and an access token.
  2. `HookParked::start(race, <the run's setter>, za_refresh(&app, &g0))`.
  3. The run's vault write answers 200.
  4. `parked.finish()`.
  5. The answer is `invalid_grant`; `raw_token_record` is `None`; the access token answers 401; `za_refresh(&app, &g0)` answers `invalid_grant`. Nothing is asserted about the grant object (spec 9.2: it may remain).

  | Run | Setter | Where the refresh waits | The vault write |
  |---|---|---|---|
  | 1 | `set_token_refresh_read_pause` | the grant has rotated; before its first record read | `password`, `STRONG` to `STRONG2` |
  | 1b | `set_token_refresh_read_pause` | the same | `recover` with the recovery key, new password `STRONG` |
  | 2 | `set_token_refresh_write_pause` | MK opened and the entries built; before its batch, which then loses its vault assertion | `password`, `STRONG` to `STRONG2` |

  `refresh_crossed_by_other_vault_write`, on `RACE`, two runs, one with each setter. Each is the run above with step 3 replaced by `za_post("recovery-key", {username: RACE, password})`, 200, a vault write that changes no credential. The answer is `Granted`; `generations` is `[0, 1]`; the new access token answers 200 and the new refresh token is granted in turn; the access token issued before the vault write answers 200 too, its keys opened again at the new revision. In the second run the batch lost its vault assertion once and the refresh wrote on its second attempt.

  After run 2 the password is changed back: the account ends at `STRONG`.

  `retry_of_g_while_g1_completes`, two runs, each on a new sign-in:
  1. Sign in: `g(0)`.
  2. `za_refresh(&app, &g0)` is granted: `g(1)`. The grant is at generation 0 with a rotation pending; the record holds 0 and 1.
  3. `HookParked::start(id, <the run's setter>, za_refresh(&app, &g0))`: the retry of `g(0)`, which upstream accepts as a retry.
  4. `za_refresh(&app, &g1)` is granted: `g(2)` and the access token `a2`. The grant is at generation 1; the record holds 1 and 2.
  5. `parked.finish()`.
  6. The retry answers `invalid_grant`; `oauth_grant` is `Some` with generation 1 (the grant is not deleted); `generations` is `[1, 2]` (generation 0 is absent; nothing put it back); `a2` answers 200; `za_refresh(&app, &g2)` is `Granted`.

  | Run | Setter | The retry at step 3 | At step 5 |
  |---|---|---|---|
  | 1 | `set_token_refresh_read_pause` | waits before its first record read | reads a record without entry 0 |
  | 2 | `set_token_refresh_write_pause` | has read the record with entry 0, opened MK and built its entries | its batch loses the record's assertion and it classifies again |

  `failed_write_then_client_retry`. Hook: `TOKEN_REFRESH_WRITE_FAIL_ONCE`.
  1. Sign in: `g(0)`.
  2. `test_hooks::set_token_refresh_write_fail_once(Some(id))`.
  3. The refresh form posted with a raw client (the answer is not a token response): a server error status.
  4. `oauth_grant` has generation 0 and `pending == true` (the grant rotated); `generations` is `[0]` and entry 0's `next` is `None` (the record did not follow).
  5. The client retries: `za_refresh(&app, &g0)` is `Granted`; its access token answers 200; `generations` is `[0, 1]`; its refresh token is granted in turn.

  `interrupted_refresh_cleared_by_another_grants_write`:
  1. Sign in (grant A): `g(0)`; `za_refresh(&app, &g0)` gives `g(1)`.
  2. `set_token_refresh_write_fail_once(Some(id))`; the refresh with `g(1)`: a server error. Grant A is at generation 1.
  3. `generations(test, id, a)` is `[0, 1]` and `oauth_grant(a).grant.generation == 1`: entry 0 is below the stored generation and nobody retries.
  4. A second sign-in of the account (grant B).
  5. `generations(test, id, a)` is `[1]` (4.5: any later write of the record drops it) and the record holds B.

- [ ] **Step 2: run them.** TM. Expected: the `tests` crate does not compile; `set_token_refresh_read_pause` is not found.

- [ ] **Step 3: add the three hooks of I §9** to `crates/common/src/za/test_hooks.rs` in the file's style, each with a table line that repeats its place.

- [ ] **Step 4: implement `za_token_refresh`.** Under `test_mode` it starts with `token_refresh_read_pause_point(account_id)`. Each of at most three attempts runs these steps in order.
  1. Read the vault record (`vault_read_usable_store`). None: `VaultChanged`. Its binding differs from the first attempt's: `VaultChanged`.
  2. Read field 153. No usable record: `Lost`. `record.binding` differs from the vault record's binding: `VaultChanged`.
  3. Read `oauth_grant` for every grant the record names; `reconcile(account_id, now(), &grants)`.
  4. `classify_refresh(account_id, grant_id, presented, secret, now())`. `Superseded` and `Lost` are returned as they are; nothing is written.
  5. With `Opened { mk, next: sealed }`: the next secret is `sealed` when it is `Some`, else `Secret::random()`. `put_refresh(account_id, grant_id, &mk, Some((presented, secret)), next, &next_secret, refresh_expires)`, then `put_access(account_id, grant_id, &mk, access_expires)`.
  6. Under `test_mode`: `token_refresh_write_pause_point(account_id)`; then, when `take_token_refresh_write_fail(account_id)`, return a `StoreEvent::UnexpectedError` with the hook's details of I §9.
  7. `token_write_store(store, account_id, Some(vault.cas), &read, Some(&record))`. Success returns `Issued` with the access secret and the next secret. An assertion failure starts the next attempt; after the third it is returned. Any other error is returned.

- [ ] **Step 5: implement `za_refresh_keyed`** (I §5.5). The expiries are computed as `za_issue_keyed` computes them.

  | `za_token_refresh` answers | `za_refresh_keyed` |
  |---|---|
  | `Issued` | appends the secrets as `za_issue_keyed` does; `Ok(Ok(()))` |
  | `Lost` | traces `Auth(AuthEvent::Failed)` with the account id, the grant id and its reason of I §5.6; deletes the grant as `za_issue_keyed` does; `Ok(Err(ErrorType::InvalidGrant))` |
  | `Superseded`, `VaultChanged` | `Ok(Err(ErrorType::InvalidGrant))`, nothing else |
  | an error | returns it: a server error, and the client's retry is upstream's retry case |

- [ ] **Step 6: the refresh grant** in `refresh_token_grant`.
  - Task 17's `Keyed` arm no longer returns: it keeps the secret of `za_split_bearer(refresh_token).1` for the next point.
  - In the `Issue` arm, after `grant_response` (`:493-502`): for a keyed refresh call `za_refresh_keyed`; an `Err(error)` returns `TokenResponse::error(error)`.

- [ ] **Step 7: run the suites.** ZA: the `tokens` module prints the ten case names of this task. SYS: `tests/src/system/oidc.rs:842-944` is upstream's refresh and rotation flow, on ordinary accounts.

- [ ] **Step 8: the closing checks.**

- [ ] **Step 9: commit.** `Release 6: refresh of keyed grants`

Catchers for Task 42: "a superseded refresh generation deletes the grant" is `retry_of_g_while_g1_completes`, run 1; "a lost record race puts back a pruned entry" is its run 2; "a token-record write omits the vault assertion", in the refresh, is `refresh_crossed_by_credential_change`, run 2.

---

### Task 19: The sweep; account destruction

**Spec:** 4.7 (expiry, a grant destroyed through the registry, an interrupted refresh, account destruction), D8, D21, invariants 67 and 69.

**Files:**
- Modify `crates/common/src/za/tokens.rs`: `za_token_sweep`.
- Modify `crates/services/src/task_manager/tasks/maintenance.rs:277-283`.
- Test: `tests/src/za/tokens.rs`.

**Interfaces:**
- Consumes: `reconcile` (I §5.3); `token_read_store`, `token_write_store` (I §5.4); `TOKEN_REFRESH_WRITE_FAIL_ONCE` (I §9); `run_purge_maintenance`, `oauth_expiry`, `za_sign_in`, `za_refresh` (I §10). Existing: the registry's account list, `self.registry().query::<RoaringBitmap>(RegistryQuery::new(ObjectType::Account))` (`maintenance.rs:182-185`); `Server::oauth_grant`.
- Produces: `za_token_sweep` (I §5.4); the row for `maintenance.rs` in I §5.5.

**Constraints:**
- The sweep holds no key and reads no vault record. Its writes assert field 153's previous bytes alone and store no wrap (D21, invariant 69).
- It reconciles as every write does: expired entries, the entries of grants that no longer exist, refresh entries below their grant's stored generation (D8, invariant 67).
- It finds records through the registry's account list with one point read per account. No range scan finds one field across accounts.
- A failure on one account is logged with the account id and its text of I §5.6, and the sweep goes on. A lost assertion on one account is skipped silently: another writer reconciled that record.
- A record that does not decode is left as it is: it may be a record of a later format (spec 4.7, D8).
- The call site logs the sweep's own error with its text of I §5.6 and does not stop the purge. The lease sweep before it (`:281-283`) is not touched. The registry's writer gets no hook (D8).
- Account destruction needs no code: the record goes with the principal's properties.

- [ ] **Step 1: write the failing za cases.** `id` is `OWNER`'s; `app` and `generations` are Task 17's and Task 18's.

  `sweep_removes_expired_entries`:
  - A sign-in under `oauth_expiry(test, Some(3), Some(3), None)` (three seconds, not one: `za_sign_in` decodes the access token it was just given), then a sleep of four seconds.
  - The record is still there (an expired entry stays until a write or the sweep); `record.open_access(id, grant, &secret, now())` is `None`; the access token answers 401.
  - After `run_purge_maintenance(test, &admin)`: `raw_token_record` is `None` (D8: the grant is left empty and dropped).
  - The grant object, which outlives its entries, is revoked.

  `registry_destroyed_grant`:
  - After a sign-in the access token answers 200.
  - `owner.registry_destroy(ObjectType::OAuthGrant, &[Id::new(grant)])` destroys it (`assert_destroyed`); the access token answers 401 at once.
  - The record still names the grant (the registry's destroy does not pass through `delete_oauth_grant`).
  - After `run_purge_maintenance`: `raw_token_record` is `None`.

  `interrupted_refresh_cleared_by_sweep`:
  1. Sign in: `g(0)`; `za_refresh(&app, &g0)` gives `g(1)`.
  2. `test_hooks::set_token_refresh_write_fail_once(Some(id))`; the refresh with `g(1)`: a server error. The grant is at generation 1; the record holds 0 and 1.
  3. `generations(test, id, grant)` is `[0, 1]`.
  4. `run_purge_maintenance(test, &admin)`.
  5. `generations` is `[1]` (D8: below the grant's stored generation) and `oauth_grant` is `Some`.

  `sweep_passes_a_broken_record`. `OWNER` holds a record with expired entries, as in `sweep_removes_expired_entries`; `JUNK` is `[1, 0xde, 0xad]`:
  - `JUNK` set in field 153 of `MFA` and of `RACE` (an account id below `OWNER`'s or above it: both orders must pass).
  - `run_purge_maintenance` returns (it waits until no task is left, failed ones included, so a failed purge would not return).
  - `raw_token_record` of `MFA` and of `RACE` is `Some(JUNK)`; of `OWNER`, `None`.
  - Both junk fields are cleared.

  `account_destruction_removes_the_record` (spec 4.7), on a key account `token-gone` that is not registered with `test.insert_account`:
  - After `za_sign_in(test, &app, GONE, STRONG, None)`, `raw_token_record(test, gone_id)` is `Some`.
  - `admin.destroy_account(gone)`, then `test.wait_for_tasks()`.
  - `raw_token_record(test, gone_id)` is `None`.

  The module's end, after its last case: revoke every grant left, drop both OAuth clients, destroy `token-plain` after `wait_for_index_queue`, clear every hook, `test.wait_for_tasks()`. `sweep::test` asserts zero UID leases on entry.

- [ ] **Step 2: run them.** ZA. Expected: FAILED in `sweep_removes_expired_entries` at "D8": the maintenance run leaves the record.

- [ ] **Step 3: implement `za_token_sweep`.** For each account id of the registry's list: `token_read_store`; no bytes, or bytes that do not decode: next account. Read `oauth_grant` for every grant the record names; `reconcile(account_id, now(), &grants)`; when it returns `true`, `token_write_store(store, account_id, None, &read, Some(&record))` and count the record. An error of this account other than a lost assertion is logged as the constraints say. An error of the account list itself is returned.

- [ ] **Step 4: the call site.** In `crates/services/src/task_manager/tasks/maintenance.rs`, after the lease sweep (`:281-283`) and before `purge_dead_nodes` (`:285`), call `server.za_token_sweep()`. An `Err` is logged with `trc::error!` and its text of I §5.6; it is not propagated. A comment cites spec 4.7 and D8.

- [ ] **Step 5: run the suite.** ZA: the `tokens` module prints the five case names, and `sweep::test`, which follows the new modules, passes.

- [ ] **Step 6: the closing checks.**

- [ ] **Step 7: commit.** `Release 6: the maintenance sweep of token records`

Catchers for Task 42: "the sweep keeps a refresh entry below its grant's stored generation" is `interrupted_refresh_cleared_by_sweep`.
