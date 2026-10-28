# Stalwart 0.16.27 update execution outcome

## Status and evidence boundary

Task 1 source inventory recorded 2026-10-28. Integration, regressions, dependency resolution,
builds, tests, binary identity, independent final review and release qualification are **pending**.
Task 1 adds source inspection only; the separate primary-run baseline receipt below is not
target qualification. Historical outcomes supply contracts, not candidate results. No runtime
or client qualification is claimed.

Execution scope is the pinned .16.27 maintenance update and required fork adaptations,
regressions, identity and documentation. No 1.0 code or private-test infrastructure, migration,
old-store reader/reset, broad crypto-test project, new backend/platform qualification, system
installation, CI dispatch, push, publication or deployment is included.

## Pinned source and isolation

| Item | Exact value |
|---|---|
| Planning/start HEAD | `9de38e09498447d23d60b52724ab8fdead91b86b` |
| Runtime fork baseline | `f9bc65b3d4f4be5acfae4737eb5fb3957d6f0857` |
| Old upstream base | `18f4e9505027d790fd7e9fdad7f922be6719101d` (v0.16.26) |
| Target tag | `v0.16.27` |
| Verified peeled target | `aa47d8ef7bfba7200d3293198ac2592747b20e73` |
| Integration branch | `codex/stalwart-01627-update` |
| Checkout | `/Users/jay/.codex/worktrees/stalwart-01627-update/stalwart` |

Primary created/reused the native isolated worktree and fetched the tag by URL. This task
read `git rev-parse HEAD v0.16.27^{}` and confirmed the values above; no fetch, merge or Git
mutation was repeated. The local v0.16.26 tag is absent, so comparisons use its pinned commit.
The start HEAD differs from the runtime baseline only in the three approved .27 planning
files (design, design-review and implementation plan); product source is unchanged.
The isolated checkout was clean before this outcome was created. Unrelated untracked work
in the original checkout remains outside this task and was not copied or inspected here.

Isolated locations (the target cache was APFS-cloned by primary; this inventory created no state):

- `CARGO_TARGET_DIR=/Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/target`
- `ZA_RUN_DIR=/Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/.run/upgrade-01627/state`
- retained command receipts: `.superpowers/sdd/2026-10-27-stalwart-01627-upgrade/receipts/`
- independent client source/venv/logs: `.run/upgrade-01627/caldav-client/`

Use stable Rust with every Cargo command prefixed by
`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`. Primary reports stable Rust/Cargo 1.99.0; this inventory
did not run toolchain probes. Integration
suites share ports and must run sequentially. Keep `STALWART_INCLUDE_SECRETS` unset.

## Primary-run unchanged baseline receipt

At start HEAD `9de38e09498447d23d60b52724ab8fdead91b86b`, primary ran
`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"; cargo test --locked -p vault -p groupware -p common`
in this isolated checkout. The retained `.superpowers/sdd/2026-10-27-stalwart-01627-upgrade/baseline-core.log`
shows common 121, groupware 149 and vault 45 passed, zero failed/ignored; build 36.67s and
suite times 1.09s/0.04s/0.01s. Primary counted 22 inherited warning lines: this is not a
warning-free claim. The log's test counts were inspected by Task 1. This is baseline evidence
only and does not satisfy post-integration candidate gates. Primary reports Docker CLI/app
unavailable; related qualification remains pending unless an existing permitted route works.

## Upstream delta and merge map

`git diff --name-only 18f4e9505027d790fd7e9fdad7f922be6719101d aa47d8ef7bfba7200d3293198ac2592747b20e73` lists 53 paths. The upstream test tree and
`.github/workflows/test.yml` remain present, and tests still enable public `test_mode` gates.
The target changes neither `crates/store/src`, `crates/types/src`, `crates/migration/src` nor
`crates/registry/src/schema/structs.rs`; generated serialization helpers change separately.
No stored-struct/schema migration is supplied by this patch.

The approved design's dry merge identified these three conflict paths. This task did not
rerun a merge or manufacture merge results; primary must record actual integration conflicts:

- `Cargo.lock`: reconcile .27 package movement with the complete fork graph.
- `crates/main/Cargo.toml`: keep the independent shipping identity and build helper.
- `crates/dav/src/common/propfind.rs`: combine `Cow`, `BTreeMap`, `Arc` imports and preserve
  fork admission, request-local memo, pending filtering, decrypted views and stored ETags.

There are 18 paths changed by both the runtime fork and the upstream patch. Excluding those
three expected conflicts, all 15 overlapping expected auto-merges must still be reviewed:

- `crates/common/Cargo.toml`
- `crates/dav/Cargo.toml`
- `crates/dav/src/principal/propfind.rs`
- `crates/email/Cargo.toml`
- `crates/groupware/Cargo.toml`
- `crates/groupware/src/calendar/itip.rs`
- `crates/http/Cargo.toml`
- `crates/imap/src/op/copy_move.rs`
- `crates/imap/src/op/search.rs`
- `crates/jmap/Cargo.toml`
- `crates/jmap/src/email/copy.rs`
- `crates/jmap/src/email/import.rs`
- `crates/jmap/src/email/set.rs`
- `crates/services/Cargo.toml`
- `tests/Cargo.toml`

The remaining upstream changes include the APPEND state notification, OAuth registration,
WebUI guard/default pin, generated secret masking helpers, DAV property schema and supplied
tests. A textual auto-merge is not correctness evidence.

### Callers and integration seams

`rg -n itip_merge_changes crates tests` identifies one definition and exactly three calls:

| Baseline location | Required .27 call |
|---|---|
| `crates/groupware/src/scheduling/inbound.rs:545` | Definition gains incoming `itip: &ICalendar` |
| `crates/groupware/src/calendar/itip.rs:788` | `itip_merge_changes(&mut event.data.event, &itip, changes)` (upstream adapts it) |
| `crates/groupware/src/calendar/drain.rs:1199` | `itip_merge_changes(&mut event.data.event, &tree, changes)` (fork adaptation) |
| `tests/src/webdav/cal_itip.rs:280` | `itip_merge_changes(ical, &message.message, changes)` (fork adaptation) |

The new `merge_timezones` copies incoming definitions only when the merged calendar references
an absent TZID, preserves an existing same-TZID definition, copies its child components and
calls `add_missing_timezones`. Preserve authenticated sender/token/pin/sequence and physical
write fences around merging, derived-time computation and resealing. A merge-producing
attendee REPLY can supply a referenced missing definition; size/quota checks remain afterward.
`calendar/seal/policy.rs` already keeps TZID, TZID parameters and VTIMEZONE X-LIC-LOCATION
visible. The planned custom timezone must use X-LIC-LOCATION:Europe/Paris with UTC fixture
fallback; no arbitrary STANDARD/DAYLIGHT parser change is authorized.

DAV upstream removes CurrentUserPrincipal from `ALL_PROPS` and `is_all_prop`, retains explicit
includes with deduplication in common/principal handlers and preserves principal PropName
behavior. Keep foreign own-id admission, eligible-only discovery, pending boundaries,
request memo/read counts, omission of private children and direct private 403 in fork paths.

The new `collect_all` argument belongs at the sole `SelectedMailbox::map_search_results`
call in `crates/imap/src/op/search.rs` (baseline call:176; definition:812 after fork insertions).
Its collection condition handles ALL and SAVE+COUNT while MIN/MAX alone save only extremes,
deduplicating a single match. Preserve key content-search refusals and trace masking.

Mail sites are content-free `PushNotification::StateChange`: APPEND uses `account_id`,
IMAP COPY uses `dest_account_id`, JMAP copy/import/set use the destination/request `account_id`.
Review JMAP copy's tracked last successful ingest change id, and set's mixed create/update/
destroy branch, for correct account and retained state signals. They do not invoke the
content-bearing EmailPush builder. Retain the existing `tests/src/za/mail/reads.rs`
`push_is_state_only` SMTP positive/negative control; no broad new push matrix is required.

### Existing fork authority contracts to preserve

Read `CLAUDE.md`, README-dev, release policy, plan 1 outcome and the sharing outcome's accepted
contract (lines 472–592). Their requirements remain binding: vault revision/CAS and cache
invalidation, keyless shared authentication and allowed session lifetimes, zeroizing secrets,
ordinary account paths, own-id authorization and fresh complete-ring authentication before
cache use, exact writer/owner/sender roles, raw None/NothingToSend/Some(empty), quiet alias
lookup, owner-only pre-finalize snapshots and separate routing overrides. Every physical
event/link mutation retains current calendar/event/vault/Account/pin fences; grant/rename
boundary retries and acknowledged counts cannot be simplified during this patch.

Untouched high-risk fork seams still need the final combined-branch review:
`common/src/cache/principals.rs`, `scheduling/event_cancel.rs`, `calendar/delegation.rs`,
both DAV `calendar/update.rs` finalize sites, cancel/SENT-BY lifecycle in `delete.rs`, DAV
URI admission/discovery/sync and `http/src/api/{vault,shares}.rs` mutation ordering.

## Changed upstream tests and key-mode equivalents (R44)

| Changed upstream file | Current execution/replacement and assertions to preserve |
|---|---|
| `tests/src/imap/search.rs` | Plain `imap_tests` runs this; key mode replaces it with `imap/za_variants.rs::search` (`imap/mod.rs:260–263`). Both baseline assertions currently expect `COUNT 10 MIN 1 MAX 10 ALL 1,10`; .27 requires `ALL 1:10`. |
| `tests/src/jmap/core/event_source.rs` | `jmap/mod.rs:226` calls it unconditionally in both modes. New import must report EmailDelivery, Email, Thread, Mailbox for the imported account, followed by ping. No key-mode replacement. |
| `tests/src/webdav/prop.rs` | `webdav/mod.rs:218` calls it in both modes. Empty allprop defines ResourceType but omits CurrentUserPrincipal/Owner/PrincipalCollectionSet; explicit include returns authenticated jane principal at root/home/collection depths. No replacement. |
| `tests/src/system/oidc.rs` | `system/mod.rs:63` runs it; this suite has no key mode. New accepted redirect URIs are `http://127.0.0.1`, `http://127.0.0.1:54321`, `http://[::1]:8080`. |

Plain search adds four SAVE regressions on its content predicate: SAVE MIN MAX ALL saves
`1 3 4 6 8 10` and reports MIN1/MAX10/ALL1,3:4,6,8,10; SAVE MIN COUNT saves the same full
set and reports COUNT6/MIN1; SAVE MIN MAX saves `1 10`; UID8 saves a single `8` with MIN8/MAX8.
The existing key variant already refuses the content predicate and uses allowed keyword
UIDs 1/3/4/6 for SAVE ALL, NOT $, size-filtered $, and MIN/MAX NOT $. It lacks the new
SAVE MIN MAX ALL regression. Task 2 must add the approved keyword expression using Flag_000,
Flag_002, Flag_003, Flag_005, asserting MIN1/MAX6/ALL1,3:4,6 and `UID SEARCH $` = `1 3 4 6`.
Preserve FROM/SUBJECT refusal coverage. The approved required addition is SAVE/MIN/MAX/ALL;
the upstream SAVE+COUNT/extremes/singleton assertions remain active in plain mode.

The new common WebUI unit is
`manager::application::tests::bundles_newer_than_the_supported_major_version_are_rejected`:
missing VERSION and 1.0.12 pass; 2.0.0, 10.1.0 and garbage refuse. Defaults pin WebUI v1.0.11
and latest-URL configurations are redirected to that pin. Take those source/tests unchanged.
Generated `registry/schema/{prelude,structs_impl}.rs` routes secrets through `secret_value`;
normal unset `STALWART_INCLUDE_SECRETS` returns masked values. Do not hand-edit or enable it.

## Archive, numeric and dependency inventory

The baseline fork and target locks both contain calcard 0.3.14 and rkyv/rkyv_derive 0.8.18.
Their calcard/rkyv lock dependency blocks are identical; rkyv_derive's syn edge changes
3.0.6 → 3.0.9. Common/groupware calcard manifests retain feature `rkyv`; rkyv consumers
retain `little_endian`. The tag does not change these declarations. Resolved Cargo feature
closure is **pending** post-merge `cargo tree`/lock reconciliation; static manifests do not
prove the resulting feature resolution.

Upstream still leaves Principal fields 150/151 unused, account flags stop at `1 << 8`, and
VanishedCollection occupies 251–254 only. The fork's `ZeroAccessVault=150`,
`ZeroAccessPins=151` (`types/src/field.rs:89,92` and conversion arms),
`ACCOUNT_IS_KEY_ACCOUNT=1 << 9` (`common/src/auth/mod.rs:111`) and
`CalendarIdentity=250` (`types/src/collection.rs:68`) remain noncolliding at these refs.
No .27 changes touch their source files or stored archive declarations.

The fork differs from .26 in exactly these 12 lock package names: argon2, chacha20,
chacha20poly1305, common, dav, email, groupware, http, jmap, stalwart, tests, vault.
Preserve all fork consumers and zeroize edges, rather than taking .27 lock wholesale.
In particular fork argon2 0.6.0, chacha20 0.10.2 and chacha20poly1305 0.11.0 include zeroize
where upstream does not; vault remains its independent 0.16.25 internal package. Vault
uses argon2/zeroize, hmac/zeroize, chacha20poly1305/zeroize, x25519-dalek with default
features off and static_secrets+zeroize, sha2 0.11, hkdf 0.13 and rkyv/little_endian.
The fork groupware manifest adds zeroize. Preserve these declarations and consumer edges.

All upstream lock version/addition movement is listed below. Versions not shown for an
existing multi-version package remain unchanged. Workspace packages are included explicitly.

| Package | .26 lock versions | .27 lock versions |
|---|---|---|
| `aws-lc-rs` | `1.18.1` | `1.18.2` |
| `aws-lc-sys` | `0.45.0` | `0.46.0` |
| `cc` | `1.6.0` | `1.8.0` |
| `combine` | `4.6.8` | `4.6.9` |
| `common` | `0.16.26` | `0.16.27` |
| `coordinator` | `0.16.26` | `0.16.27` |
| `dav` | `0.16.26` | `0.16.27` |
| `dav-proto` | `0.16.26` | `0.16.27` |
| `deranged` | `0.5.8` | `0.5.9` |
| `directory` | `0.16.26` | `0.16.27` |
| `dns-update` | `0.5.9` | `0.6.0` |
| `email` | `0.16.26` | `0.16.27` |
| `equivalent` | `1.0.2` | `1.0.3` |
| `event_macro` | `0.16.26` | `0.16.27` |
| `groupware` | `0.16.26` | `0.16.27` |
| `http` | `0.16.26, 1.5.0` | `0.16.27, 1.5.0` |
| `http_proto` | `0.16.26` | `0.16.27` |
| `imap` | `0.16.26` | `0.16.27` |
| `imap_proto` | `0.16.26` | `0.16.27` |
| `inout` | `0.1.4, 0.2.2` | `0.1.4, 0.2.3` |
| `jmap` | `0.16.26` | `0.16.27` |
| `jmap_proto` | `0.16.26` | `0.16.27` |
| `libz-sys` | `1.1.29` | `1.1.30` |
| `managesieve` | `0.16.26` | `0.16.27` |
| `migration` | `0.16.26` | `0.16.27` |
| `nlp` | `0.16.26` | `0.16.27` |
| `pest` | `2.9.2` | `2.9.3` |
| `pest_derive` | `2.9.2` | `2.9.3` |
| `pest_generator` | `2.9.2` | `2.9.3` |
| `pest_meta` | `2.9.2` | `2.9.3` |
| `pop3` | `0.16.26` | `0.16.27` |
| `psm` | `absent` | `0.1.24` |
| `registry` | `0.16.26` | `0.16.27` |
| `rustls` | `0.23.45` | `0.23.46` |
| `rustls-native-certs` | `0.8.4` | `0.8.5` |
| `schemars` | `0.9.0, 1.2.2` | `0.9.0, 1.2.3` |
| `schemars_derive` | `1.2.2` | `1.2.3` |
| `scim` | `0.16.26` | `0.16.27` |
| `scim-proto` | `0.16.26` | `0.16.27` |
| `sequoia-openpgp` | `2.4.1` | `2.4.2` |
| `serde_json` | `1.0.151` | `1.0.154` |
| `services` | `0.16.26` | `0.16.27` |
| `sha2` | `0.9.9, 0.10.9, 0.11.0` | `0.9.9, 0.10.9, 0.11.1` |
| `smallvec` | `1.16.2` | `1.16.3` |
| `smtp` | `0.16.26` | `0.16.27` |
| `spam-filter` | `0.16.26` | `0.16.27` |
| `stacker` | `absent` | `0.1.25` |
| `stalwart` | `0.16.26` | `0.16.27` |
| `store` | `0.16.26` | `0.16.27` |
| `syn` | `2.0.119, 3.0.6` | `2.0.119, 3.0.9` |
| `tests` | `0.16.26` | `0.16.27` |
| `tokio` | `1.53.2` | `1.53.3` |
| `tokio-util` | `0.7.19` | `0.7.20` |
| `toml_edit` | `0.25.16+spec-1.1.0` | `0.25.17+spec-1.1.0` |
| `toml_parser` | `1.1.4+spec-1.1.0` | `1.1.5+spec-1.1.0` |
| `trc` | `0.16.26` | `0.16.27` |
| `types` | `0.16.26` | `0.16.27` |
| `unicode-ident` | `1.0.26` | `1.0.27` |
| `utils` | `0.16.26` | `0.16.27` |
| `uuid` | `1.27.0` | `1.28.0` |
| `zerocopy` | `0.7.35, 0.8.61` | `0.7.35, 0.8.63` |
| `zerocopy-derive` | `0.7.35, 0.8.61` | `0.7.35, 0.8.63` |
| `zopfli` | `0.8.3` | `0.8.4` |
| `zstd-sys` | `2.1.0+zstd.1.5.7` | `2.1.1+zstd.1.5.7` |

The following same-version upstream lock blocks also change dependency edges. This covers
movement not visible in the version table (removed → added edges only):

| Package/version | Removed edges | Added edges |
|---|---|---|
| `aead 0.6.1` | `inout 0.2.2` | `inout 0.2.3` |
| `ahash 0.8.12` | `zerocopy 0.8.61` | `zerocopy 0.8.63` |
| `async-recursion 1.2.0` | `syn 3.0.6` | `syn 3.0.9` |
| `async-trait 0.1.92` | `syn 3.0.6` | `syn 3.0.9` |
| `bytecheck_derive 0.8.3` | `syn 3.0.6` | `syn 3.0.9` |
| `cipher 0.5.2` | `inout 0.2.2` | `inout 0.2.3` |
| `darling_core 0.24.1` | `syn 3.0.6` | `syn 3.0.9` |
| `darling_macro 0.24.1` | `syn 3.0.6` | `syn 3.0.9` |
| `displaydoc 0.2.7` | `syn 3.0.6` | `syn 3.0.9` |
| `futures-macro 0.3.34` | `syn 3.0.6` | `syn 3.0.9` |
| `lzma-rust2 0.16.5` | `sha2 0.11.0` | `sha2 0.11.1` |
| `pbkdf2 0.13.0` | `sha2 0.11.0` | `sha2 0.11.1` |
| `postgres-protocol 0.6.12` | `sha2 0.11.0` | `sha2 0.11.1` |
| `ppv-lite86 0.2.21` | `zerocopy 0.8.61` | `zerocopy 0.8.63` |
| `proc-macro-error3 3.1.1` | `syn 3.0.6` | `syn 3.0.9` |
| `ptr_meta_derive 0.3.2` | `syn 3.0.6` | `syn 3.0.9` |
| `ref-cast-impl 1.0.27` | `syn 3.0.6` | `syn 3.0.9` |
| `rkyv_derive 0.8.18` | `syn 3.0.6` | `syn 3.0.9` |
| `scrypt 0.12.0` | `sha2 0.11.0` | `sha2 0.11.1` |
| `serde_derive 1.0.229` | `syn 3.0.6` | `syn 3.0.9` |
| `serde_derive_internals 0.30.0` | `syn 3.0.6` | `syn 3.0.9` |
| `serde_repr 0.1.21` | `syn 3.0.6` | `syn 3.0.9` |
| `serde_with 3.24.0` | `schemars 1.2.2` | `schemars 1.2.3` |
| `serde_with_macros 3.24.0` | `syn 3.0.6` | `syn 3.0.9` |
| `serial_test_derive 4.0.1` | `syn 3.0.6` | `syn 3.0.9` |
| `synstructure 0.14.0` | `syn 3.0.6` | `syn 3.0.9` |
| `thiserror-impl 2.0.21` | `syn 3.0.6` | `syn 3.0.9` |
| `tokio-macros 2.7.2` | `syn 3.0.6` | `syn 3.0.9` |
| `totp-rs 6.0.0` | `sha2 0.11.0` | `sha2 0.11.1` |
| `wasm-bindgen-macro-support 0.2.129` | `syn 3.0.6` | `syn 3.0.9` |
| `yoke-derive 0.8.4` | `syn 3.0.6` | `syn 3.0.9` |
| `zenoh-keyexpr 1.10.1` | `schemars 1.2.2` | `schemars 1.2.3` |
| `zenoh-util 1.10.1` | `schemars 1.2.2` | `schemars 1.2.3` |
| `zerofrom-derive 0.1.8` | `syn 3.0.6` | `syn 3.0.9` |
| `zerovec-derive 0.11.6` | `syn 3.0.6` | `syn 3.0.9` |

Direct manifests change dns-update from 0.5 to 0.6 in common, services and tests; other
upstream manifest movement is package .16.26 → .16.27. Pest 2.9.3 introduces stacker 0.1.25
and psm 0.1.24 (both use cc; stacker also uses libc/cfg-if/windows-sys). Explicit crypto/runtime
movement includes sha2 0.11.0 → 0.11.1, inout 0.2.2 → 0.2.3, rustls/aws-lc,
sequoia-openpgp and Tokio/tokio-util shown above. Fixed archive versions do not freeze the
crypto graph. Preserve existing crypto units and functional gates; residual patch-dependency
risk is accepted for this undeployed preview's scope. Fixed SHA-256 fingerprint tests are
not X25519 known-answer tests. No cross-version read, KDF/AEAD compatibility or rollback
claim follows; no mandatory new crypto project or old-store migration gate is added.

## Qualification receipts — pending

All rows below are unexecuted for this candidate. Prefix every Cargo command with the PATH
line above and use the isolated CARGO_TARGET_DIR. Set STORE=RocksDb and
RUST_MIN_STACK=16777216 for integration; clear ZA_KEY_ACCOUNTS for plain mode and set 1 only
for key mode. Set BLOB_STORE=FileSystem only for the named ZA variant. Keep commands sequential.
Record source SHA, environment, selected nonzero counts, named-case completion and retained
failure/retry receipts before changing any row to passed.

| Gate | Exact selector/command after integration | Status |
|---|---|---|
| Pre-merge timezone red | Complete `za::za_tests` with approved fixture; fail at semantic assertion, not setup | Pending |
| Component compile | `cargo check -p stalwart --no-default-features --features rocks`; `cargo test -p tests --no-run` | Pending |
| Style | baseline/working/index `git diff --check`; `cargo fmt --all --check` | Pending |
| Core units | `cargo test -p vault -p groupware -p common` | Pending |
| HTTP | `cargo test -p http@0.16.27 --features test_mode` | Pending |
| Edited crates | `cargo test -p imap -p jmap -p dav -p dav-proto` | Pending |
| Protocol/store | `cargo test -p jmap_proto`; `cargo test -p imap_proto`; `cargo test -p store` | Pending |
| ZA embedded | `cargo test -p tests za::za_tests -- --exact --nocapture` | Pending |
| ZA filesystem | Same selector with BLOB_STORE=FileSystem | Pending |
| WebDAV | `cargo test -p tests webdav::webdav_tests -- --exact --nocapture`, plain/key | Pending |
| IMAP | `cargo test -p tests imap::imap_tests -- --exact --nocapture`, plain/key | Pending |
| JMAP | `cargo test -p tests jmap::jmap_tests -- --exact --nocapture`, plain/key | Pending |
| OAuth/OIDC | `cargo test -p tests system::system_tests -- --exact --nocapture` | Pending |
| Directory/SMTP | Existing workflow setup/selectors/pinned SPAM_RULES_URL; no invented smtp_tests suite | Pending |
| OSS product | `cargo build --locked --release -p stalwart --no-default-features --features rocks` | Pending |
| Release metadata | `python3 -B scripts/tests/test_release_metadata.py` | Pending |
| Verifier units | `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v` | Pending |
| Independent CalDAV harness | `python3 -B -m unittest discover -s tests/caldav_client -p test_runner.py -v` | Pending |
| Independent CalDAV | Existing runner.py, exact pinned python-caldav 111ae14b875f40d100d5259820f10210f00e20f9, default two modes, isolated logs | Pending |
| Actual macOS binary identity | Existing load_metadata/verify_checkout/validate_identity helpers, clean committed source and clear CONFIG_PATH; never Linux CLI on macOS | Pending |
| Final whole-branch review | Fresh independent read-only agent, pinned base/head and accepted contract | Pending |

Preserve one documented unchanged-source retry for cal_itip/plain JMAP timing flakes and the
system diagnostic rerun, retaining failures. Unit compile failures remain failed/pending until
an equivalent baseline reproduction or conclusive source evidence; unaffected units cannot
satisfy failed gates. No passing suite may be relabeled as execution on a later changed source.

Native Apple Calendar macOS/iOS, Thunderbird and Mail observations remain pending on disposable
state under the manual checklist. SQL/other stores, multi-node, additional platforms, exact-source
Linux shipping artifact verification, CI and publication remain separate pending gates.

## Integration and final handoff placeholders

- Actual merge commit/conflicts and reviewed auto-merges: pending.
- Red fixture receipt, three caller changes, timezone/DAV/search regressions: pending.
- Reconciled lock/feature graph and dependency disposition: pending.
- beta.2 metadata, upstream .27 identity and clean source/binary agreement: pending.
- Commands, counts, warnings, failures/retries and final review adjudication: pending.
- Reviewed locally qualified branch and remaining gates: pending.
