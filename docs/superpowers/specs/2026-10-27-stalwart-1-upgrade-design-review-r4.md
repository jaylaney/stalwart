# Review of the Stalwart 1.0.1 upgrade design and plan, revision 4

Reviewed on 2026-10-30 against `d04af6152` on `stalwart-1-upgrade-plan`, fork `main` at
`9a7574203`, `v1.0.1` (`23b69755c`), the previous reviews (revision 3 review, findings
R3-H1 to R3-L8; Codex "Revision 3 re-review", C3-1 to C3-6) and the calendar design spec.
No build or test was run. Jay's four answers and the maintain-upstream principle are taken
as given; a finding below reports only where the text that carries them out contradicts the
source, a binding spec, or itself.

This is the narrow check that was asked for: the dead-property design against 1.0.1's
metadata store, every line revision 4 changed, and verdicts on the revision 3 findings.
Line numbers refer to the files at `d04af6152`: the design
`docs/superpowers/specs/2026-10-27-stalwart-1-upgrade-design.md` and the plan
`docs/superpowers/plans/2026-10-27-stalwart-1-upgrade.md`.

## Summary

| Severity | Count |
|---|---:|
| Critical | 0 |
| High | 1 |
| Medium | 4 |
| Low | 6 |

Revision 3 findings: 18 resolved, 3 partly resolved, 0 not resolved.

Revision 4 describes the metadata store correctly now (classes, where names live, the
bookkeeping records, the object kinds, the key). What is still wrong or missing is in how
the decision is carried out: the sealed container must stay a parseable upstream container
because six keyless paths parse it (R4-M1); the seal and unseal sites are DAV's consumers,
not the keyless common functions the plan names (R4-M2); DAV COPY/MOVE copies the container
bytes and must re-key them (R4-M3); and the swap tests cannot reconfigure the suite's single
server (R4-M4). The one high finding is the privacy-flag disposition (C3-2): upstream's
privacy filters read the flags from the keyless DAV resource cache at ten sites, so
"derived on the keyed view before upstream's filters run" is not where the filters look;
the real choice is to persist two classification bits or to accept that CLASS-based
hiding is inert on key-account calendars, and that choice is Jay's (R4-H1).

## Verdicts on the revision 3 findings

| Finding | Verdict | What is left |
|---|---|---|
| R3-H1 store described wrongly | partly resolved | All four points are corrected (classes, names in the value, object kinds, the key). The new text leaves the implementation sites and the wrapper format wrong or unstated: R4-M1, R4-M2, R4-M3. |
| R3-M1 `#[path]` collision | resolved | Per-module file name; the `mod.rs`/`lib.rs`/`main.rs` case is right; mapping recorded in Task 1. |
| R3-M2 `common` needs `store`/`types` | resolved | Task 2 closes with `cargo check -p types -p store -p vault`; the named hooks are the right ones. |
| R3-M3 R54 grantee clause; no terminal failure | resolved | Owner-only count; remedy named. One nuance on what `set` does: R4-L1. |
| R3-M4 `Value`/`Presence` not subspaces | resolved | The five new subspaces and the three reassigned bytes match `store/src/lib.rs:137-162` and the fork's constants. |
| R3-M5 swap default `Disabled` | partly resolved | The default is now stated correctly; the tests' way of configuring `LocalFile`/`BlobStore` is not executable on the suite's one server: R4-M4. |
| R3-M6 "final region"; sync sub-tests | resolved | Both cases defined; one ambiguity remains in R4-L5, which the early-return self-test would expose. |
| R3-L1 LIST RETURN (METADATA) | resolved | `imap/src/op/list.rs:181-182` named. |
| R3-L2 refusal order | resolved | "after `directory.authenticate` returns and before `assert_account_directory`", lines correct. |
| R3-L3 stale Task 9 line | resolved | |
| R3-L4 which ACL wait | resolved | Identified by its comment; the other three stay. |
| R3-L5 `rebuild_shares` | resolved | "runs unchanged"; matches `rebuild_shares.rs:95-121`. |
| R3-L6 R58 layering | resolved | Worker attaches, `account_batch` places; `store` cannot call `common`. |
| R3-L7 `DefaultAlertsResolver` | resolved | Listed with the viewer-side readers, with the right call sites. |
| R3-L8 slot wording | resolved | |
| C3-1 bookkeeping vs containers | resolved | `Viewer`/`Owner` stay upstream's 12-byte records; leak rule permits them. |
| C3-2 CLASS through privacy flags | partly resolved | Non-persistence is settled, but the stated mechanism contradicts where upstream reads the flags: R4-H1. |
| C3-3 names as well as values | resolved | The sealed unit is the container, names and values together. |
| C3-4 distinct restored paths | resolved | |
| C3-5 restored tests in the cohort | resolved | Frozen 40-file cohort plus listed new fork code; restored tests and `tests` reported apart. |
| C3-6 synchronous sub-tests | resolved | |

## The dead-property design against 1.0.1

What revision 4 states and the source confirms:

- Keys: `account / Collection::Metadata / class / collection / document_id`, the viewer id
  in `Private` keys, no property name (`v1.0.1:crates/store/src/write/key.rs:590-608`).
- Containers: `Shared` and `Private { viewer }` only
  (`v1.0.1:crates/common/src/storage/metadata/mod.rs`, the `batch.set(MetadataClass::…)`
  arms of `MetadataWrite::build` and `PrivateMetadataWrite::build`). The value is
  `Archive::serialize_raw(EncodedMetadata bytes)` with a hashed trailer
  (`store/src/write/metadata.rs`, `StoredMetadata::new`), and the inner bytes are a
  container of `Entry { key, value }` (`types/src/metadata/entries.rs`).
- Bookkeeping: `Viewer`/`Owner` are 12-byte `ViewerState { change_id, containers }`
  (`VIEWER_STATE_LEN = U64_LEN + U32_LEN`), written by `merge_viewer_state`, decoded
  keylessly by `read.rs:152-193`, `purge.rs` and `viewer.rs`.
- Object kinds: eight (`jmap/src/api/metadata/mod.rs:94-103`).
- DAV writes only `Shared`: `dead/patch.rs:190` checks `MetadataScope::Shared`, and
  `into_private_write` has no caller under `crates/dav`. DAV reads `Shared` through
  `DeadContainers::load` → `load_metadata_containers` and
  `metadata_container(…, MetadataClass::Shared)`.
- Presence flags are derived at write time from the plaintext `EncodedMetadata`
  (`MetadataPresence::new(previous, next)`, `mod.rs:139-144`) and from the stored header
  on read (`MetadataBuf::kinds()`), then merged into the object's archive by a store-side
  merge (`groupware/src/metadata/mod.rs:40-60`, `rewrite_archive`). Nothing here needs the
  container's entries.
- The changelog carries ids and a prefix, not content (`store/src/write/batch.rs:484-540`).
- The write assertion compares the stored trailer hash
  (`store/src/write/assert.rs:75-77`), so it works on sealed bytes as on plain ones.
- The leak rule ("every container of a key account's calendar or event is sealed; the
  12-byte records are permitted; not a name check") is consistent with all of the above.

What revision 4 gets wrong or leaves out is in the findings below.

## New findings

### High

#### R4-H1. The privacy-flag disposition names a mechanism upstream's filters do not use

- **Where:** design C6 (line 124) and "Privacy flags" (lines 281–286); plan Task 4 (lines
  410–416); dispositions table (line 851, C3-2).
- **Claim:** "For a key account those flags are never persisted in the sanitized metadata
  row. They are derived on the keyed, unsealed view before upstream's filters run."
- **Evidence:** every upstream consumer of the two bits reads them from the DAV resource
  cache, not from a view: `EventPrivacy::from_flags(resource.event_flags())` at
  `v1.0.1:crates/dav/src/calendar/{copy_move.rs:154,178, freebusy.rs:153, get.rs:82,
  mod.rs:130,294,343}`, `dav/src/common/{lock.rs:587, propfind/dead.rs:53,
  propfind/item.rs:105}`, and `flags & EVENT_SECRET` at `dav/src/calendar/query.rs:117`
  and `dav/src/common/propfind/get.rs:111,609`. `event_flags()` returns
  `GroupwareResourceMetadata::CalendarEvent { flags }` (`common/src/storage/dav/resource.rs:326-328`),
  which `storage/dav/store.rs` builds keylessly from the stored metadata rows and the swap
  tier persists (`cache/swap/resources.rs`). The bits themselves are set only by
  `refresh_from_content` (`groupware/src/calendar/index.rs:247`,
  `content.data.event.privacy().apply_to_flags(self.flags)`), which on a sealed tree sees
  no `CLASS` and clears them; so "never persisted" holds by construction, and the filters
  then treat every event of a key-account calendar as public. 0.16.27 has no privacy flag
  at all (`git grep EVENT_PRIVATE v0.16.27 -- crates` is empty), so inert filtering is
  parity with 0.16. Making the filters see a derived value "before they run" would mean
  fork code at the ten sites above, or a keyed override of the resource cache, which the
  maintain-upstream principle rules out.
- **Severity:** high (the text as written cannot be carried out, and the consequence is a
  visibility decision). **Confidence:** high.
- **Suggested change:** replace the paragraph with the binary choice and record Jay's
  answer: (a) parity with 0.16: `CLASS` stays sealed, the two bits stay clear, and
  CLASS-based hiding does not apply to a key account's shared calendars (grantees hold the
  collection key anyway); or (b) persist `EVENT_PRIVATE`/`EVENT_SECRET` for key accounts as
  a visibility-policy change with its own version, so upstream's filters work unchanged.
  Under (a), the Task 4 "`CLASS:PRIVATE` and `CLASS:CONFIDENTIAL` grantee test" asserts that
  the grantee sees the event and that no flag is stored; under (b) it asserts upstream's
  hiding. Either way, remove "derived on the keyed, unsealed view before upstream's filters
  run".

### Medium

#### R4-M1. The sealed container must still parse as an upstream container; the marker cannot be the first byte

- **Where:** design lines 177–187 ("Sealed unit … names and values together … Marker: a
  fork payload version byte marks a sealed container"); plan Task 4 (lines 398–400).
- **Evidence:** six keyless paths parse the inner bytes: `MetadataBuf::read` requires
  `MetadataView::from_trusted(&archive.inner).is_some()` (`store/src/write/metadata.rs`,
  `read`), `StoredMetadata::view` likewise, `StoredContainer::new(&view, value)` takes
  `view.kinds()` (`common/src/storage/metadata/mod.rs:109-115`), `containers_of` and
  `all_containers_of` call `StoredMetadata::view` on every row (`read.rs:239,274`),
  `container_kinds` (`dav/src/common/dead/containers.rs:320-322`) and the cleanup, purge
  and quota paths load containers the same way. `from_trusted` reads the header
  `[FORMAT_VERSION = 1, kinds bits, entry count ≤ len]` (`types/src/metadata/view.rs:62-69,
  175-181`); a value that starts with a fork version byte fails it with
  `DataCorruption("Invalid metadata container header")`. The presence derivation also needs
  the real kinds in that header.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** state the wrapper: a valid container whose header carries the real
  kinds and whose single entry is a `Dav` entry (`EntryKind::Dav`, `view.rs:111-135`) under
  a fork-reserved namespace and name, with the ciphertext as its value; the fork's version
  byte goes at the head of that value. Add to the Task 4 tests that `MetadataBuf::read`,
  `container_kinds`, `destroy_owner_metadata`, `purge_private_metadata` and
  `metadata_used_quota` run over sealed containers without keys.

#### R4-M2. The seal and unseal sites are DAV's consumers, not the keyless common functions the plan names

- **Where:** plan Task 4 (lines 391–393: "Do this in the common write and read path of
  `common/src/storage/metadata/`, which `dav/src/common/dead/` uses. Do not add a fork
  path."); design line 189.
- **Evidence:** `MetadataWrite::build(self, document_id, batch)` and
  `PrivateMetadataWrite::build` take no keys and receive `next: EncodedMetadata` already
  built; `containers_of`, `all_containers_of` and `metadata_container` take no keys and also
  serve `cleanup.rs`, `purge.rs`, `quota.rs`, `containers.rs` and the presence code, which
  must stay keyless. The request's keys are present in `dead/patch.rs` where `next` is built
  (`prepare`, lines 122–230, `change.into_write(…)` at 230) and in the readers
  `propfind/dead.rs` (`dead_properties`, `dead_property_names`, which call `.view()` at
  lines 118–140) and `dead/containers.rs` (`get`, `stored_entries`). A subagent following
  the plan would look for a keys handle in `storage/metadata/` and find none.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** "seal `next` in `dead/patch.rs` before `into_write`, and unseal in
  `propfind/dead.rs` and `dead/containers.rs` after `.view()`, each a one-line call into a
  fork module; `common/src/storage/metadata/` and `store/src/write/metadata.rs` stay
  upstream's and keyless".

#### R4-M3. DAV COPY and MOVE copy the container bytes, so a sealed container must be re-keyed

- **Where:** design "Dead properties" (lines 177–187) and "Calendar and sharing writes"
  (line 210, which lists COPY/MOVE for content only); plan Task 4 (lines 391–403).
- **Evidence:** `DeadContainers::copy(source: &MetadataBuf)` rebuilds the destination's
  container from `EncodedMetadata::from_view(&source.view())`
  (`v1.0.1:crates/dav/src/common/dead/containers.rs:135-150`), that is, from the stored
  bytes as they are. Between calendars the collection key differs, so a copied sealed
  container would be unreadable at its destination. The calendar spec requires the unseal
  and reseal on a UID-changing copy and keeps cross-account copies refused (lines 655–658),
  and the fork already routes such copies through `za_copy_move_event`
  (`crates/dav/src/calendar/copy_move.rs:1403`).
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** add to Task 4: "`DeadContainers::copy` re-seals under the
  destination calendar's key inside `za_copy_move_event`'s path; add a COPY and a MOVE
  across two calendars to the round trips".

#### R4-M4. The swap tests cannot configure `LocalFile`/`BlobStore` on the suite's single server

- **Where:** design "Cache snapshots" (lines 360–364); plan Task 2 interfaces (lines
  225–229) and Task 6 (lines 568–571, `tests/src/za/cache_swap.rs`).
- **Evidence:** the tier is built once, from the registry, in `Caches::parse` →
  `SwapTier::build(bp, cache.swap, storage)` (`common/src/config/inner.rs:125`), and
  `cache/reload.rs` has no swap handling (no `swap` in the file), so a running server cannot
  switch from `Disabled` to `LocalFile`. The za suite is one server on port 8899
  (CLAUDE.md), and the plan places `cache_swap.rs` inside it.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** say that `cache_swap.rs` boots its own `TestServer` (own state
  directory and port) with `cache.swap` set in its bootstrap, twice (`LocalFile`, then
  `BlobStore` under `BLOB_STORE=FileSystem`), and that the suite's server keeps `Disabled`.

### Low

#### R4-L1. `x:IndexQueueStatus/set` resets a partition's status; it does not drop entries

- **Where:** design lines 340–343; plan Task 6 (lines 528–534) and Task 9 (lines 771–774).
- **Evidence:** `index_queue_status_set` patches the status object and, when the status
  changes, `batch.clear(partition.status_class())`
  (`v1.0.1:crates/jmap/src/registry/mapping/index_queue.rs:191-240`): the backoff is
  cleared and the partition retries at once. A document that fails to build every time
  (a corrupt row) fails again. The entry drains only when the document is deleted (its
  entry becomes an un-index).
- **Severity:** low. **Confidence:** high.
- **Suggested change:** word the remedy as "reset the partition with
  `x:IndexQueueStatus/set` for a transient failure; delete the offending document for a
  permanent one", and have the Task 6 test use a fault hook that fails once.

#### R4-L2. The amended C6 forbids an exception the calendar spec already grants

- **Where:** design C6 (line 124: "or through a stored flag derived from it").
- **Evidence:** the spec computes the email-alarm from the alarm's text before sealing
  (calendar spec lines 646–647), and the fork does so at
  `crates/dav/src/calendar/update.rs:270-288`; 1.0.1 derives `EVENT_HAS_ALARMS` and
  `EVENT_USES_DEFAULT_ALERTS` from content as well (`groupware/src/calendar/index.rs:240-246`).
- **Severity:** low. **Confidence:** high.
- **Suggested change:** "… or through a stored flag derived from it, beyond the alarm flags
  the calendar spec allows (section 9)".

#### R4-L3. The `Private` discussion is vacuous for key accounts; the leak rule can be stricter

- **Where:** design lines 180–183; plan Task 4 (lines 398–400) and Task 6 (lines 592–594).
- **Evidence:** DAV writes only `Shared` (`dead/patch.rs:190`; no DAV caller of
  `into_private_write`), and the other writers of `Private` (IMAP METADATA, JMAP object
  metadata) are refused for key accounts. So no `Private` container can exist for a key
  account's calendar or event. "Other object kinds" also omits Email, reachable only through
  refused JMAP metadata.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** state that `Private` containers never exist for a key account's
  calendars and events, make the leak rule "no `Private` container; every `Shared`
  container sealed", and add Email to the refused-only kinds.

#### R4-L4. The presence rewrite is a metadata-only rewrite worth listing

- **Where:** design lines 195–197 ("Presence flags … stay upstream's"); plan Task 4
  (`split_metadata_only_update_keeps_sealed_content`, lines 377–379).
- **Evidence:** after a dead-property write, `rewrite_presence` deserializes and
  re-serializes the `CalendarEvent` metadata archive through a store-side merge to flip
  the presence bits (`v1.0.1:crates/groupware/src/metadata/mod.rs:40-60`,
  `common/src/storage/index/presence.rs:50`). It touches the sanitized row only, so it is
  compatible with C6, but it is exactly the class of write that test guards.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** have `split_metadata_only_update_keeps_sealed_content` include a
  PROPPATCH that flips a presence bit and scan the row afterwards.

#### R4-L5. "The region that ends at the body's closing brace" also describes the whole-body region

- **Where:** design lines 491–493; plan Task 1 (line 160) and Task 7 (line 620).
- **Evidence:** in llvm-cov's export the outermost region of a body spans from the opening
  to the closing brace and counts entries, so it also "ends at the closing brace" and is
  positive after an early return. The intended region is the exit region at the brace
  itself. The early-return self-test (plan line 657) would expose a tool that picked the
  wrong one.
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** "the exit region whose start and end both lie on the closing
  brace, not the body region that starts at the opening brace".

#### R4-L6. The plan's dead-property tests do not cover the keyless paths

- **Where:** plan Task 4 (lines 401–404).
- **Evidence:** the listed round trips are all keyed (owner, grantee, corrupted container,
  counters). The keyless readers in R4-M1 and the delete paths
  (`container_cleanup_from`, `destroy_owner_metadata`, `metadata_used_quota`,
  `purge_private_metadata`) are what a wrong wrapper breaks.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** add "delete an event and a calendar holding sealed containers;
  destroy the account; check quota" to the tests, keyless.

## Questions for Jay

1. **Privacy filtering on key-account calendars (R4-H1).** Parity with 0.16 (no
   CLASS-based hiding for grantees of a sealed calendar; `CLASS` stays sealed), or persist
   the two classification bits as a visibility-policy change so upstream's filters work?
2. **Swap tests (R4-M4).** Is a second, short-lived `TestServer` inside the za suite
   acceptable for `cache_swap.rs`, or should the swap cases move to their own suite function
   so CI can run them as a separate step?

## Verified in revision 4

- Directory refusal order and lines (`authentication.rs:246-250, 467`).
- `rebuild_shares` writes only `acl_grant`/`acl_revoke` (`rebuild_shares.rs:95-121`).
- No DAV write of event preferences at 1.0.1 (`mkcol.rs:114` is calendar-level;
  `copy_move.rs:716-717, 1022-1023` retain the caller's own).
- `DefaultAlertsResolver` call sites (`get.rs:180`, `copy_move.rs:682, 726, 987, 1032`).
- PR #66's wait identified by its comment; `TestServer::cleanup` does not wait.
- `TaskShareRevokeCleanup` holds only the owner; grantee-side writes are counters.
- Retry backoff `MIN_RETRY_DELAY = 30`, `MAX_RETRY_DELAY = 30 * 60`, no terminal state;
  `IndexQueueStatus` is a registry struct (`structs.rs:3426`).
- `account_batch` has no commit point; the R58 layering (worker attaches, store places).
- `CacheSwap` default `Disabled`; `SwapTier::build` returns `disabled()` for it.
- The 26-variant `Subspace` enum, the five new subspaces and the three reassigned bytes.
- `#[path]` resolution and the 20 colliding directories; the per-module naming rule.
- The sync sub-tests (`imap::body_structure::test`, `webdav::cal_itip::test`).
- `MetadataScope::{Shared, Private}`; `EntryKind::{JmapVendor, JmapRegistered, Dav, Imap}`;
  `MetadataKinds::{JMAP, DAV, IMAP}`; `FORMAT_VERSION = 1`.
