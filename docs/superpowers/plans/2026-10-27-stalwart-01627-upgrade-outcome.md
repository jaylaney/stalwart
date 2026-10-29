# Stalwart 0.16.27 update execution outcome

## Status and evidence boundary

The current candidate version is `0.1.1-beta.1`; see the
[version follow-up](#version-follow-up-on-2026-10-29). Earlier binary identities and runtime
results below retain their original versions and source commits.

The [system-suite follow-up](#system-suite-follow-up-on-2026-10-28) records the later
fixture corrections, two passing complete system runs and fresh CalDAV qualification.
Docker-dependent directory, Linux release-artifact and native-client qualification remain
pending. The initial candidate snapshot below retains its original failures and source attribution.

### Initial candidate snapshot

Task 1 source inventory was recorded on 2026-10-28; the retained source inventory below is
historical context. The .27 merge and beta.2 preparation are integrated. Candidate
`51bbecce06cd0875d09639b6fbe427253856a379` passed the recorded style, 412 unit tests and eight
whole integration-suite rows. SMTP met its existing bounded retry contract. The complete
system suite remains failed/pending after its one permitted unchanged-source diagnostic
retry; this is a bounded local qualification, not an all-green branch or release acceptance.
The exact locked OSS release build, clean candidate macOS identity, independent harness
and both pinned CalDAV modes passed with the retained skip/xfail policy.
Docker-dependent directory, Linux artifact and native-client gates remain pending.

Implementation and the authorized bounded local execution are complete. The complete system
gate and external/native qualifications remain outstanding; this is not full release
qualification. A later outcome-only commit requires a fresh clean-final-HEAD identity
build/check. Earlier tests keep their exact source attribution, and no final SHA or final
identity result is implied here.

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

Primary created/reused the native isolated worktree and fetched the tag by URL.
Task 1 historical inspection read `git rev-parse HEAD v0.16.27^{}` and confirmed the values above; no fetch, merge or Git
mutation was repeated. The local v0.16.26 tag is absent, so comparisons use its pinned commit.
The start HEAD differs from the runtime baseline only in the three approved .27 planning
files (design, design-review and implementation plan); product source is unchanged.
The isolated checkout was clean before this outcome was created. Unrelated untracked work
in the original checkout remains outside this task and was not copied or inspected here.

Isolated locations (the target cache was APFS-cloned by primary; this inventory created no state):

- `CARGO_TARGET_DIR=/Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/target`
- Actual Task 4 per-row `TMPDIR=/Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/.run/upgrade-01627/state/<row>`; Task 2 used `.run/task2-red/` and `.run/task2-green/` as its receipts record
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

## Task 1 historical source inventory — upstream delta and merge map

`git diff --name-only 18f4e9505027d790fd7e9fdad7f922be6719101d aa47d8ef7bfba7200d3293198ac2592747b20e73` lists 53 paths. The upstream test tree and
`.github/workflows/test.yml` remain present, and tests still enable public `test_mode` gates.
The target changes neither `crates/store/src`, `crates/types/src`, `crates/migration/src` nor
`crates/registry/src/schema/structs.rs`; generated serialization helpers change separately.
No stored-struct/schema migration is supplied by this patch.

The approved design's dry merge identified these three conflict paths. Task 1 did not
rerun a merge; the later actual integration dispositions are recorded below:

- `Cargo.lock`: reconcile .27 package movement with the complete fork graph.
- `crates/main/Cargo.toml`: keep the independent shipping identity and build helper.
- `crates/dav/src/common/propfind.rs`: combine `Cow`, `BTreeMap`, `Arc` imports and preserve
  fork admission, request-local memo, pending filtering, decrypted views and stored ETags.

There are 18 paths changed by both the runtime fork and the upstream patch. Excluding those
three expected conflicts, the 15 overlapping auto-merges requiring later review were:

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

The inventory identified these untouched high-risk fork seams for the later combined review:
`common/src/cache/principals.rs`, `scheduling/event_cancel.rs`, `calendar/delegation.rs`,
both DAV `calendar/update.rs` finalize sites, cancel/SENT-BY lifecycle in `delete.rs`, DAV
URI admission/discovery/sync and `http/src/api/{vault,shares}.rs` mutation ordering.

## Task 1 historical inventory — changed upstream tests and key-mode equivalents (R44)

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
UIDs 1/3/4/6 for SAVE ALL, NOT $, size-filtered $, and MIN/MAX NOT $. At inventory time it lacked the new
SAVE MIN MAX ALL regression. The Task 2 requirement was to add the approved keyword expression using Flag_000,
Flag_002, Flag_003, Flag_005, asserting MIN1/MAX6/ALL1,3:4,6 and `UID SEARCH $` = `1 3 4 6`.
Preserve FROM/SUBJECT refusal coverage. The approved required addition is SAVE/MIN/MAX/ALL;
the upstream SAVE+COUNT/extremes/singleton assertions remain active in plain mode.

The new common WebUI unit is
`manager::application::tests::bundles_newer_than_the_supported_major_version_are_rejected`:
missing VERSION and 1.0.12 pass; 2.0.0, 10.1.0 and garbage refuse. Defaults pin WebUI v1.0.11
and latest-URL configurations are redirected to that pin. Take those source/tests unchanged.
Generated `registry/schema/{prelude,structs_impl}.rs` routes secrets through `secret_value`;
normal unset `STALWART_INCLUDE_SECRETS` returns masked values. Do not hand-edit or enable it.

## Task 1 historical inventory — archive, numeric and dependency inventory

The baseline fork and target locks both contain calcard 0.3.14 and rkyv/rkyv_derive 0.8.18.
Their calcard/rkyv lock dependency blocks are identical; rkyv_derive's syn edge changes
3.0.6 → 3.0.9. Common/groupware calcard manifests retain feature `rkyv`; rkyv consumers
retain `little_endian`. The tag does not change these declarations. At inventory time, resolved Cargo feature closure was pending; the later lock/metadata
resolution is recorded below. Static manifests alone did not prove that resolution.

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

## Integration, regressions and dependency disposition

The primary committed Task 1 inventory as `65c6fd54d`, the expected-failing regression
checkpoint as `f8e00a3d5`, the pinned target merge as
`4a4b124382dcecb07101a8784d4f323f7338c648`, and beta.2 preparation as
`51bbecce06cd0875d09639b6fbe427253856a379`. Actual conflicts were exactly `Cargo.lock`,
`crates/main/Cargo.toml` and `crates/dav/src/common/propfind.rs`. DAV retained the combined
Cow/BTreeMap/Arc import and fork admission/memo/filter/decrypted-view/stored-ETag paths.
The primary inspected the supplied auto-merges and required fork adaptations; focused
Task 1, 2 and 3 reviews approved their scopes with no findings.

All three incoming-calendar callers now pass the appropriate calendar: upstream `&itip`,
fork drain `&tree`, fixture helper `&message.message`. The new regression's accepted .26
RED receipts show repeated existing-zone controls at 13:00Z with REPORT `[true,false]`,
and repeated missing-zone delivery at 14:00Z with REPORT `[false,true]`. Both REPORT
windows lie strictly inside each cached resource interval. ZA failed only at the delayed
semantic assertion after recurrence, sealing and cleanup observations; WebDAV failed only
on the missing expected VTIMEZONE. Earlier setup/compile failures are retained separately
and do not count as RED. Receipts: `task2-red-za-final.log`, `task2-red-webdav.log`; exact
argv/environments and setup corrections are retained in `task-2-report.md` and
`receipts/task2-red-commands.md`.

Task 2 GREEN whole ZA and WebDAV plain/key suites passed against the uncommitted merged
source before the merge commit. All repeated deliveries have one timezone definition,
13:00Z, REPORT `[true,false]`, one master/override, three expanded 30-minute instances,
original recurrence identity, resealing and protected-canary absence. Shared discovery
executes displayname, empty allprop, include and PropName with single-read assertions,
private omission/direct refusal and authenticated-reader principal. Task 4's eight whole
suite passes below reran this behavior on the beta.2 candidate. The plain IMAP upstream
SAVE/COUNT/extremes/singleton cases and new key flag SAVE MIN MAX ALL/saved-set case ran;
content predicate refusals remain active. Both JMAP modes execute the supplied import
EmailDelivery state test. Common's WebUI version guard executed in core units. OIDC,
including .27 portless loopback registration, completed on the diagnostic system retry;
this submodule completion does not pass the complete system gate.

Task 2 product `cargo check --locked -p stalwart --no-default-features --features rocks`
passed (1m59s); `cargo test --locked -p tests --no-run` passed (3m25s). Task 2 GREEN receipts
are `task2-check-product.log`, `task2-tests-no-run.log`, `task2-green-za.log`,
`task2-green-webdav-plain.log`, `task2-green-webdav-key.log` and `task2-format-final.log`.
Their source attribution remains the evolving merge at checkpoint `f8e00a3d5`, not beta.2.

Lock reconciliation retained the fork graph. One offline metadata normalization advanced
vault's sha2 edge to the target's supplied 0.11.1; final locked metadata succeeded.
Normalized package blocks show exactly the twelve preexisting fork package-name deviations
listed in the inventory, with no extra registry version/source/checksum deviation from .27.
Resolved calcard/rkyv archive features and argon2/chacha20/chacha20poly1305/hmac zeroize,
x25519-dalek static_secrets+zeroize/default-features-off remain present. Vault stays
0.16.25; shipping package alone advances independently to 0.1.0-beta.2. See
`task2-lock-normalized-final.json`, `task2-resolved-features.json`,
`task2-lock-validation-final.log`, `task2-lock-assertions.txt`. These are graph/source and
functional evidence, not cross-version archive/KDF/AEAD compatibility or migration proof.

## Qualification receipts and exact source attribution

Receipt paths below are relative to
`.superpowers/sdd/2026-10-27-stalwart-01627-upgrade/receipts/` in the isolated checkout.
Task 4 JSON records source SHA, exact argv, duration, result counts, warning-line counts
and log paths. Each log begins with its command/environment receipt. All completed Task 4
rows below ran at `51bbecce06cd0875d09639b6fbe427253856a379`; no source changes intervened.
Every Cargo invocation prepended `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` and used
worktree-local target/state. `STALWART_INCLUDE_SECRETS`, `STALWART_BUILD_SOURCE_COMMIT`,
`CONFIG_PATH`, `ZA_KEY_ACCOUNTS` and `BLOB_STORE` were cleared before named overrides.
Integration rows used `STORE=RocksDb RUST_MIN_STACK=16777216`; key rows alone set
`ZA_KEY_ACCOUNTS=1`, and filesystem ZA alone set `BLOB_STORE=FileSystem`.
TMPDIR paths are unique row directories under `.run/upgrade-01627/state/`; Task 3's final
Python metadata run instead uses outside-workspace `/private/tmp` fixture isolation.
Commands ran sequentially. No zero-selected row is counted as a passed gate.

### Task 4 units and style

| Gate | Exact argv | Result | Seconds | Raw warning lines | Receipt |
|---|---|---|---:|---:|---|
| style-diff | `git diff --check f9bc65b3d4f4be5acfae4737eb5fb3957d6f0857 HEAD` | exit 0 | 0.05 | 0 | `task4-style-diff.log` |
| style-fmt | `cargo fmt --all --check` | exit 0 | 3.41 | 0 | `task4-style-fmt.log` |
| core | `cargo test --locked -p vault -p groupware -p common` | 317 passed, 0 failed | 46.35 | 22 | `task4-core.log` |
| http | `cargo test --locked -p http@0.16.27 --features test_mode` | 6 passed, 0 failed | 61.57 | 28 | `task4-http.log` |
| edited-crates | `cargo test --locked -p imap -p jmap -p dav -p dav-proto` | 23 passed, 0 failed | 54.34 | 29 | `task4-edited-crates.log` |
| jmap-proto | `cargo test --locked -p jmap_proto` | 16 passed, 0 failed | 27.77 | 0 | `task4-jmap-proto.log` |
| imap-proto | `cargo test --locked -p imap_proto` | 45 passed, 0 failed | 9.52 | 0 | `task4-imap-proto.log` |
| store | `cargo test --locked -p store` | 5 passed, 0 failed | 23.77 | 3 | `task4-store.log` |

Total: 412 actual unit tests. The zero-test doctest rows in the raw JSON are excluded
from this total and do not provide additional coverage. Warning figures count raw output
lines per command, including repeated diagnostics/summaries; they are not unique warning
counts and do not establish a warning-free build. Machine summary: `task4-unit-results.json`.

### Task 4 whole integration rows and bounded retries

All whole-suite commands use `cargo test --locked -p tests SELECTOR -- --exact --nocapture`.
The SMTP filter and exact failed-name retry deliberately retain their existing workflow argv.

| Gate | Selector / overrides | Result | Seconds | Raw warning lines | Receipt |
|---|---|---|---:|---:|---|
| za-embedded | `za::za_tests` | 1 passed, 0 failed; exit 0 | 438.92 | 2 | `task4-za-embedded.log` |
| za-filesystem | `za::za_tests; BLOB_STORE=FileSystem` | 1 passed, 0 failed; exit 0 | 399.72 | 2 | `task4-za-filesystem.log` |
| webdav-plain | `webdav::webdav_tests` | 1 passed, 0 failed; exit 0 | 69.8 | 2 | `task4-webdav-plain.log` |
| webdav-key | `webdav::webdav_tests; ZA_KEY_ACCOUNTS=1` | 1 passed, 0 failed; exit 0 | 21.02 | 2 | `task4-webdav-key.log` |
| imap-plain | `imap::imap_tests` | 1 passed, 0 failed; exit 0 | 10.44 | 2 | `task4-imap-plain.log` |
| imap-key | `imap::imap_tests; ZA_KEY_ACCOUNTS=1` | 1 passed, 0 failed; exit 0 | 12.34 | 2 | `task4-imap-key.log` |
| jmap-plain | `jmap::jmap_tests` | 1 passed, 0 failed; exit 0 | 185.34 | 2 | `task4-jmap-plain.log` |
| jmap-key | `jmap::jmap_tests; ZA_KEY_ACCOUNTS=1` | 1 passed, 0 failed; exit 0 | 171.82 | 2 | `task4-jmap-key.log` |
| system | `system::system_tests` | 0 passed, 1 failed; exit 101 | 11.76 | 2 | `task4-system.log` |
| system-retry1 | `system::system_tests; RUST_BACKTRACE=1` | 0 passed, 1 failed; exit 101 | 20.95 | 2 | `task4-system-retry1.log` |
| smtp | `cargo test --locked -p tests smtp -- --nocapture; SPAM_RULES_URL=https://github.com/stalwartlabs/spam-filter/releases/download/v3.0.3/spam-filter-rules.json.gz` | 71 passed, 5 failed; exit 101 | 67.17 | 2 | `task4-smtp.log` |
| smtp-retry1 | `cargo test --locked -p tests -- --exact --nocapture smtp::inbound::dkim2::dkim2_all_undisclosed_do_not_leak smtp::outbound::lmtp::lmtp_delivery smtp::queue::retry::queue_retry smtp::reporting::analyze::report_analyze smtp::reporting::dmarc::report_dmarc` | 5 passed, 0 failed; exit 0 | 9.09 | 2 | `task4-smtp-retry1.log` |

Eight complete ZA/WebDAV/IMAP/JMAP rows passed, each selecting one whole suite; their
internal cases are not miscounted as individual Cargo-selected tests. Both system attempts
selected one suite and failed. The first failed in OIDC; the sole permitted diagnostic
retry (additional `RUST_BACKTRACE=1`) completed OIDC then failed in security. Full system
qualification remains failed/pending and later submodules remain unqualified. No further
candidate rerun, skip, feature or assertion change was used. Relevant unchanged source is
not an unchanged-baseline runtime reproduction; no runtime root cause or upgrade attribution
is claimed. The accepted bounded disposition preserves these receipts without broadening
this maintenance update into fixture repair or an upstream bug-report project.

SMTP initially passed 71/76 and failed five; its single existing-workflow failed-name retry
passed all 5/5 unchanged-source cases. This meets the existing retry contract, not a claim
that the complete filter passed in one attempt. The five exact names and argv remain in
`task4-integration-results.json` and `task4-smtp-retry1.log`. Both SMTP runs use pinned
`SPAM_RULES_URL=https://github.com/stalwartlabs/spam-filter/releases/download/v3.0.3/spam-filter-rules.json.gz`.
Original failures and local-only diagnostic disposition are retained. No test source changed.

### Identity, independent client and outstanding gates

| Gate | Evidence / exact command or requirement | Status |
|---|---|---|
| Release metadata | `python3 -B scripts/tests/test_release_metadata.py`; `task3-metadata.log` | Passed 8/8 before metadata commit; 5.776s |
| Product verifier units | `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v`; `task3-verifier.log` | Passed 13/13 before metadata commit; 0.363s |
| OSS product | `cargo build --locked --release -p stalwart --no-default-features --features rocks` | Passed, exit 0; 1165.46s; 28 raw warning lines; `task4-release-build.log` |
| Actual initial macOS identity | Existing `load_metadata`, `verify_checkout`, `validate_identity` against actual clean candidate HEAD/branch; clear CONFIG_PATH and combine help stdout/stderr | Passed at clean candidate `51bbecce06cd0875d09639b6fbe427253856a379`; Mach-O arm64; `task4-initial-identity.json` |
| Independent harness | `python3 -B -m unittest discover -s tests/caldav_client -p test_runner.py -v` in isolated venv | Passed 30/30; 5.223s suite / 5.62s command; `task4-caldav-harness.log` |
| Independent CalDAV | `python3 -B tests/caldav_client/runner.py --binary PATH --caldav-source PATH --log-dir PATH`; unfiltered existing runner, both default modes | Passed both modes; plain 66 passed / 1 skipped / 5 xfailed (6.19s), key 71 passed / 1 skipped / 5 xfailed (7.56s); 21.41s command; `task4-caldav-client.log` |
| Directory workflow | Existing Docker/testcontainers environment | Pending; Docker CLI/app/standard sockets unavailable, DOCKER_HOST unset |
| Whole-branch review | Fresh clean-context read-only review of `9de38e09498447d23d60b52724ab8fdead91b86b..51bbecce06cd0875d09639b6fbe427253856a379` | No actionable source blocker; separate from runtime proof |
| Final handoff identity | Rebuild exact locked OSS product after the outcome-only commit, then validate actual clean final HEAD/branch using macOS helpers | Required; pending `task4-final-identity.json`; no final SHA invented |
| Exact-source Linux artifact | Existing shipping workflow, ELF ABI/CPU/GLIBC inspection, checksum/attestation | Pending; no CI dispatch authorized |
| Native Calendar/Mail/Thunderbird | Disposable-state manual checklist, macOS/iOS where applicable | Pending; automated results do not qualify native clients |

Task 3's first metadata invocation used a worktree-contained TMPDIR and produced one fixture
isolation error out of eight; `task3-metadata-in-workspace-tmp.log` is retained. The corrected
full run used unique outside-workspace `/private/tmp` state and passed without code change.
The Python passes establish metadata/verifier behavior on that Task 3 source; they are not
relabeled as executions after future source changes or as actual product-binary identity.

The primary completed the product/client batch against clean source
`51bbecce06cd0875d09639b6fbe427253856a379`. Machine summary:
`task4-product-client-results.json`. The pinned python-caldav checkout HEAD is exactly
`111ae14b875f40d100d5259820f10210f00e20f9`; the unfiltered collection manifests and existing
compatibility/skip/xfail policies were unchanged. Both default modes passed under those
policies; skipped and expected-failure cases are not counted as passes.

Exact product/client argv from the completed receipt (absolute paths preserve the actual
isolated interpreter, product, client source and retained client-log directories):

```sh
cargo build --locked --release -p stalwart --no-default-features --features rocks
/Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/.run/upgrade-01627/caldav-client/venv/bin/python -B -m unittest discover -s tests/caldav_client -p test_runner.py -v
/Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/.run/upgrade-01627/caldav-client/venv/bin/python -B tests/caldav_client/runner.py --binary /Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/target/release/stalwart --caldav-source /Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/.run/upgrade-01627/caldav-client/source --log-dir /Users/jay/.codex/worktrees/stalwart-01627-update/stalwart/.run/upgrade-01627/caldav-client/candidate-51bbecce0-logs
```

Each command used its named `.run/upgrade-01627/state/<row>` TMPDIR; secrets opt-in and
release-profile environment overrides were unset. The release command used the required
Cargo PATH and normal release optimization, with no test-mode/enterprise features.
The release log contains 28 raw warning lines, not 28 unique diagnostics or a warning-free
claim. The harness/client command logs contain zero raw warning-prefix lines; their
skip/xfail counts are separately retained above.

`task4-initial-identity.json` records the actual candidate binary's validated version and
help first line, clean checkout, branch `refs/heads/codex/stalwart-01627-update`, fork
`0.1.0-beta.2`, upstream tag `v0.16.27` and pinned upstream commit. It identifies a Darwin
Mach-O 64-bit arm64 executable with SHA-256
`1c9f51dfc203916294a2cbe1cee9d9f55b82bfa7bbf8505dd0b6b3918a082c63`.
The primary used `load_metadata(root)`, `verify_checkout(root, source_commit)` and
`validate_identity(metadata, version_output, help_output, source_commit, branch_ref)`;
CONFIG_PATH was cleared and help stderr captured with stdout. This is macOS identity only,
not Linux artifact/ABI/CPU/GLIBC/provenance qualification.

The outcome-only commit will change embedded identity even though runtime source is
unchanged. Primary must rebuild the exact locked OSS product against its actual clean
final HEAD/branch and retain `task4-final-identity.json`. That final commit SHA and identity
result remain pending at this record's preparation; no completed candidate runtime result
is relabeled as execution on that future documentation commit.

## Review, routing and handoff boundary

The fresh final reviewer independently inspected the whole branch with base/head refs and
accepted contracts, without implementer reports or previous findings. No actionable source
blocker was found. Focused Task 1/2/3 reviews likewise approved their scopes. Primary owns
actual-diff adjudication and final receipt verification. Source review does not establish
native/platform behavior or cure the failed system gate.

The originating model identity/capability ceiling was unavailable. Agents deliberately
inherited the parent route rather than guessing a ranking, with medium/high/xhigh reasoning
for evidence/implementation/adversarial roles as supported. This is the routing limitation;
no max/ultra route or nested delegation was used during this implementation run.

SQL/other stores, multi-node, additional platforms, new crypto or cross-version archive
compatibility, old-store readers/migrations and binary-only rollback safety remain
unestablished. Historical outcomes are contracts, not new candidate evidence. The reviewed
maintenance source is prepared locally with the qualification limits above. There has been
no push, PR, CI dispatch, publication, deployment, old-data access/reset, system installation
or worktree cleanup. Reviewer-owned design-review and historical outcomes remain untouched.

## System-suite follow-up on 2026-10-28

The user reopened the failed system gate, requested the full CalDAV end-to-end suite and
a pull request to the fork. The follow-up code changes are confined to four system-test
fixtures. Product source, dependency versions, stored layouts, credentials/key handling,
CalDAV collection manifests and compatibility policies are unchanged from `0d03497f3`.

The fixture corrections preserve the original behavioral checks:

- OAuth refresh checks use the original token's recorded expiry, a ten-second fixture
  lifetime and a four-second renewal window. Early refresh must omit renewal, late refresh
  must renew, and the original token must fail at expiry. Access-token expiry and the
  existing grant, refusal, one-use-code and loopback checks remain.
- The temporary-ban phase isolates login/IP counters, reuses a client and requires 100
  failures with 401, the triggering 429, correct-credential 403 while the five-second ban
  is active, and 200 after recorded expiry and reload. Permanent-ban and concurrency checks
  remain.
- Temporary upload limits are captured before quota checks, restored through exactly
  `UploadTtl`, `UploadQuota` and `MaxUploadCount`, reloaded and read back before later
  imports. TTL readback compares milliseconds. Upload count/bytes/expiry and account disk
  quota checks remain.
- Task tests retain live retries through the exact maximum of three. Explicit timestamps
  check initial scheduling, deadline equality, delayed/beyond-deadline rejection and the
  attempt limit. An already-expired retry fixture still traverses the real queue, lock and
  worker, then must retain its identity/origin, fail at attempt one with the simulated
  failure reason, become terminal and be removed.

Local diagnostics captured both authentication checks running at an expired deadline. A
new restoration assertion also failed deterministically before the upload-setting fix,
showing `(50000, 3, 1)` instead of `(50000000, 1000, 3600)` in its original seconds-based
readback. The earlier task-count failure lacks its original timing data; deterministic
boundary checks establish the valid delayed-retry semantics. The missing blob's reservation
expiry was consistent with the failure time, with no exact server access-check timestamp.

All commands below ran sequentially on macOS with stable Rust 1.99.0, locked dependencies,
fresh worktree-specific state, `STORE=RocksDb`, `RUST_MIN_STACK=16777216`, and secrets/source
identity overrides unset. Each system/WebDAV selector selected one complete Cargo test suite.
Normal mode left `ZA_KEY_ACCOUNTS` unset; the encrypted WebDAV row set it to `1`.

| Gate | Result | Command seconds | Receipt |
| --- | --- | ---: | --- |
| `cargo fmt --all --check` | exit 0 | 3.44 | `style-final.log` |
| `system::system_tests`, repeat 1 | 1 passed, 0 failed; exit 0 | 114.25 | `system-final1.log` |
| `system::system_tests`, repeat 2 | 1 passed, 0 failed; exit 0 | 66.32 | `system-final2.log` |
| `webdav::webdav_tests`, ordinary | 1 passed, 0 failed; exit 0 | 17.03 | `webdav-plain.log` |
| `webdav::webdav_tests`, key accounts | 1 passed, 0 failed; exit 0 | 20.86 | `webdav-key.log` |
| `cargo test --locked -p vault -p groupware -p common` | 317 unit tests passed; exit 0 | 12.90 | `core-units.log` |
| CalDAV harness | 30 passed; exit 0 | 5.23 | `caldav-harness.log` |
| Full pinned CalDAV runner, both modes | ordinary 66 passed / 1 skipped / 5 xfailed; key 71 passed / 1 skipped / 5 xfailed; exit 0 | 23.73 | `caldav-client.log` |

The 317 core units are common 123, groupware 149 and vault 45; zero-test doctests are
excluded. CalDAV still requires all 72 ordinary and 77 key-account cases, including strict
flows. Existing skips/expected failures remain distinct from passes.

The Rust rows identify parent `0d03497f3325c363288b8ff820ed660d8b1b23ee` plus frozen patch
SHA-256 `6f1d211974d4a64059c6122d913c2d472f9aa90a0b4c216fb4c9182ec4318e3f`.
The subsequent TTL readback precision correction changes only two `as_secs()` calls to
`as_millis()` and their assertion label. A final committed-head system check is required before
opening the PR; retain its exact-source receipt in the follow-up execution record.

The independent CalDAV run reused the verified clean OSS binary from
`0d03497f3325c363288b8ff820ed660d8b1b23ee`, SHA-256
`ed24c2e3196a475e3d1bae20040ad83aed0ce3450ae2d14a038eb27c1cf6878d`.
Its `--version`/`--help` identity and checksum were checked again; shipping inputs and the
client runner are unchanged by these test-only corrections. Python 3.15.2 executed the
existing pinned client commit `111ae14b875f40d100d5259820f10210f00e20f9`. This run establishes
fresh local CalDAV behavior for that artifact; it is not a rebuilt artifact for the later
PR head or Linux release provenance.

Exact commands, environments, source patches, initial failures and sanitized client logs
remain under `.superpowers/sdd/2026-10-28-system-suite-repair/` and
`.run/system-suite-repair/caldav-client-logs/`. A fresh independent review covered the
four test-file corrections; the originating model tier was unavailable, so delegated work
inherited the parent model with high reasoning for implementation and xhigh for review.
Review files remain untouched. Publication/deployment and native, Docker-dependent directory
or Linux release-artifact acceptance are separate gates.

## Version follow-up on 2026-10-29

The user selected `0.1.1-beta.1` for the current .27 candidate. Release metadata, the
shipping manifest/lock entry, metadata test expectation and current candidate documentation
now agree on that version. Upstream provenance remains `v0.16.27` /
`aa47d8ef7bfba7200d3293198ac2592747b20e73`; other lockfile package blocks are unchanged.

Fresh checks passed: eight release-metadata tests, thirteen product-verifier tests, locked
offline Cargo metadata and workspace formatting. The earlier system and CalDAV results
keep their recorded source/artifact identities. A shipping binary has not been rebuilt for
this version-only change; its exact version/commit/provenance must be verified when built
before publication. The published `fork-v0.1.0-beta.1` tag and artifacts remain immutable.

## Fork-owned review follow-up on 2026-10-29

The user limited this follow-up to fork-owned review findings. Key-account IMAP coverage
now includes SAVE/MIN/COUNT, SAVE/MIN/MAX extrema and singleton deduplication, in addition
to the existing SAVE/MIN/MAX/ALL case. Each asserts the exact saved UID set using allowed
keyword/UID predicates; existing content-search refusals remain.

The seeded deadline test drops its redundant elapsed-time assertion and retains task
identity, changed failure reason, retry count, creation time, terminal state and cleanup.
Test wording now explains upstream's existing convention of three retries after the initial
execution. Production code, version/provenance metadata and review files are unchanged by
this follow-up; inherited upstream findings were left outside its scope.

Formatting and the complete IMAP key-account, ordinary-account and System suites passed.
Each suite selected one whole Cargo test with zero failures. Command durations were 12.41s,
10.03s and 62.96s respectively. The first sandboxed IMAP attempt failed during listener
setup; the same source passed after local-listener permissions were granted. Exact commands,
environments and receipts remain in `.superpowers/sdd/2026-10-29-fork-review-fixes/`.
Runtime receipts identify parent `cdbc668d6bf5552f780fa848a6351f4e1c33471a` plus source patch
SHA-256 `4a344c825937d78c9bb373093635e7e6b6eb339c7aab22eae4723c602adbbb38`.
An independent focused source review passed for conformance and test correctness.

## OpenTelemetry source follow-up on 2026-10-29

The four OpenTelemetry Git declarations now use the original project's
`https://github.com/open-telemetry/opentelemetry-rust` URL and explicitly pin the existing
commit `ae66e97b140f70e477ab710686aafce665cc2f8b`. Six lockfile source identities change to
match. All 942 package records, versions, checksums, dependency edges and manifest features
are otherwise unchanged. The fork-specific OpenTelemetry source remains identical.

A fresh anonymous Git fetch and a complete `cargo fetch --locked` from an initially empty
Cargo home retrieved that commit, including its protobuf submodule. The dependency's Git
tree is `2741d63ab90f580e29f781180abebca90c1ce22b`, matching the previously tested cached
source; its tracked checkout is clean. The Cargo fetch passed in 19.64s, followed by locked
offline Cargo metadata in 0.46s. The fetch and metadata left Cargo.lock unchanged.
Workspace formatting and exact normalized manifest/lock comparisons passed.
An independent focused review of the manifest/lock patch passed for conformance and
correctness with no findings.

These checks establish source retrieval and dependency resolution. Earlier runtime and
artifact results retain their recorded source identities; no shipping binary is rebuilt by
this source-URL correction. Commands, downloaded-source identity and comparison receipts
are retained under `.superpowers/sdd/2026-10-29-otel-source-fix/`. The updated PR runs the
existing CI workflow; no test scope, manifest exception policy or workflow is changed.
