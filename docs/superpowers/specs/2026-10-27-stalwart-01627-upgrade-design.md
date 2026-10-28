# Stalwart 0.16.27 fork update design

## Decision and scope

Update the fork to upstream `v0.16.27` while reconsidering the 1.0 port and its private-test
policy. Jay selected planning this maintenance update. Revised 2026-10-28 after review.
The separate 1.0 design and plan
remain deferred reference material; no part of that port is included here.

Starting fork: `f9bc65b3d4f4be5acfae4737eb5fb3957d6f0857`, based on upstream `v0.16.26`
at `18f4e9505027d790fd7e9fdad7f922be6719101d`. Target: `v0.16.27`, commit
`aa47d8ef7bfba7200d3293198ac2592747b20e73`.

Merge the immutable tag normally into an isolated fork worktree. Preserve published history,
fork enforcement modules and the public test infrastructure. The inspected target changes
53 paths; a dry merge against the starting fork has three conflicting paths: `Cargo.lock`,
`crates/main/Cargo.toml`, and `crates/dav/src/common/propfind.rs`. These counts describe the
starting refs, not a guarantee that a later fork tip has the same conflicts.

The alternative was proceeding directly to 1.0, which replaces persistence/runtime interfaces
and removes the public tests. The selected .27 update retains the current architecture and
testing setup with a much smaller integration surface. Staying on .26 was also considered;
.27 includes useful IMAP, iTIP, OAuth and DAV corrections. This choice does not establish
that upstream will continue maintaining the 0.16 series.

## Preserved contracts

All existing approved account, sealing, invitation, pinning, mail and sharing designs remain
binding, including the accepted interpretations in the
[sharing outcome](../plans/2026-10-23-zero-access-plan14-outcome.md).

- Preserve vault revision authority and `za_commit`/`za_vault_commit`, credential edit
  refusals, session-key lifetimes, generation/cache fencing and trace redaction.
- Preserve stored struct layouts, crypto/KDF/AAD inputs and encrypted carriers. Calcard
  remains `0.3.14` and rkyv remains `0.8.18`; check their resolved features and archived
  types during lockfile reconciliation. No format change is proposed.
- Preserve Principal fields 150/151, key-account flag `1 << 9`, vanished collection 250,
  visibility policy, physical event/link fences and current sharing authorization.
- Ordinary accounts take the updated upstream behavior. Key-account content/search/copy
  refusals and state-only push remain effective around the auto-merged notification code.
- Keep public `tests/`, its feature gates, all ZA modules, plain/key protocol modes and
  existing CI jobs. Do not import the 1.0 private-test layout or build a new test framework.
- Shipping remains RocksDB without enterprise or test mode. Qualification remains the
  existing single-node/store/platform scope; native client evidence stays separate.

## Integration decisions

### DAV property selection

Resolve the import conflict with
`use std::{borrow::Cow, collections::BTreeMap, sync::Arc};`.
Adopt .27's `allprop` behavior in the common/principal handlers and `dav-proto` property
selection: `current-user-principal` is returned when explicitly requested/included rather
than injected into the default property set. Preserve explicit includes without duplicates.

Retain the fork's fresh per-calendar admission, request-local context memo, pending-item
filtering, decrypted response views, stored ETags, assisted discovery and private-resource
omission/refusal. Do not take either entire handler file as the conflict resolution.

### iTIP merge and timezone handling

Use the new signature
`itip_merge_changes(ical: &mut ICalendar, itip: &ICalendar, changes: Vec<MergeAction>)`.
The merged caller in `calendar/itip.rs` passes its incoming `&itip`. Adapt the additional
callers explicitly: `calendar/drain.rs` passes `&tree`, and
`tests/src/webdav/cal_itip.rs` passes `&message.message`.

The incoming timezone definitions must survive authenticated drain, merge and resealing.
Add a recurring-event update that introduces a custom `VTIMEZONE`, moves one occurrence,
and is delivered again: one timezone definition, the correct recurrence identity/time, no
duplicate occurrence and no protected canaries in stored data. Include a no-new-timezone
control whose stored event already defines every referenced TZID. Keep pin, token, sequence,
sender and write-fence checks unchanged.

Calcard 0.3.14 resolves timezone names and recognized aliases; it does not execute arbitrary
STANDARD/DAYLIGHT rules to resolve an unknown custom TZID. Use a custom TZID with
`X-LIC-LOCATION:Europe/Paris`, a UTC fixture default and a winter date for the time-range
regression. After drain, query a narrow window around the moved occurrence's correct UTC
time and an incorrect fallback-time window; the first matches and the second does not.
This checks stored derived times and calendar-query behavior. The drain computes those
times before sealing; background calendar content indexing remains refused for key accounts.
Record the developer guide's visibility-policy check: TZID and X-LIC-LOCATION remain visible.

Adopt .27's bounded timezone behavior: recognized missing TZIDs may gain generated
definitions, affecting stored size and ETag, with the existing size/quota checks afterward.
A merge-producing, authenticated attendee REPLY may supply a referenced missing definition;
it cannot replace an existing same-TZID definition. Participant/token/sequence checks remain
unchanged. No broader scheduling or timezone-parser change is proposed.

### Remaining upstream changes

Accept IMAP `SEARCH RETURN`/SAVE corrections and the `collect_all` argument at its local
call site; preserve key content-search refusals and trace masking. Update the key-mode
search variant to expect `ALL 1:10` and mirror SAVE/MIN/MAX/ALL using its permitted keyword
criteria. Audit every changed upstream test against any key-mode replacement.

Accept `EmailDelivery` state notifications in APPEND/COPY and JMAP create/import/copy paths.
These sites already construct content-free `StateChange`; they do not enter the `EmailPush`
content builder. Retain the existing SMTP content-push privacy/ordinary-account control and
the supplied import EventSource regression in both modes. No broad new mock-push privacy
matrix is needed; review delivery to the correct account and preservation of state changes.

Take the supplied registry-generated changes without hand edits. The added
`STALWART_INCLUDE_SECRETS` migration opt-in stays unset for normal operation and qualification;
there is no reason to enable it for this update. Retain the .27 WebUI 1.x pin/version guard
and its tests, rather than allowing a 1.0-server-only WebUI to auto-update onto this base.

### Dependency reconciliation

Inventory the supplied dependency movement, including `sha2` 0.11.0 to 0.11.1 and `inout`
0.2.2 to 0.2.3 in the vault's crypto chain; the rustls/aws-lc, OpenPGP and Tokio updates;
`dns-update` 0.5 to 0.6; and pest's new stacker/psm dependencies. Calcard/rkyv staying fixed
does not mean the complete crypto dependency graph is fixed.

Use .27's lockfile as a reconciliation baseline if useful, then retain the complete fork
manifest/feature graph and inspect Cargo's resulting package blocks without a blanket
dependency update. The starting fork differs from .26 in 12 package names: `argon2`,
`chacha20`, `chacha20poly1305`, `common`, `dav`, `email`, `groupware`, `http`, `jmap`,
`stalwart`, `tests`, `vault`. Preserve their required consumer and zeroize edges; a rule
allowing changes only to `stalwart` and `vault` would discard legitimate fork dependencies.

For this undeployed preview, the proposed assurance is a dependency/feature audit plus
the existing crypto units and functional gates, with the remaining patch-dependency risk
recorded. Fixed SHA-256 fingerprint checks already exist; they are not X25519 known-answer
tests and do not establish full KDF/AEAD compatibility. No cross-version data test or new
HKDF/Argon2/AEAD test project is required for this update, and no backward-read claim follows.

### Product identity

Propose the next compatible preview as `0.1.0-beta.2`, with `fork-v0.1.0-beta.2` and title
`release-0.1.0-beta-2`, subject to release review. Keep the shipping package version matched
to `fork-release.toml`; take .27's internal package versions without globally renumbering
fork-owned crates. Record the exact upstream tag/SHA and a new fork source identity.
Never change the published beta.1 tag or artifacts.

## Qualification and authority

The [implementation plan](../plans/2026-10-27-stalwart-01627-upgrade.md) covers the regression
cases, sequential existing test net, OSS product build, independent review and provenance.
Require a fresh whole-branch review of the combined merge and adaptations. Record exact
source/commands/counts, failures/retries and outstanding native/store/platform gates.

The fork has no deployed data. Use fresh disposable state; no migration, backward reader,
old-state deletion, 1.0 infrastructure or new platform matrix is needed. Unchanged layouts
do not by themselves promise an existing-installation upgrade or rollback contract.

This is a proposed design for review. Planning creates no implementation branch, runtime
changes, build/test results, CI run, publication or deployment. Publishing and live actions
remain separate authorization steps under the [release policy](../../releases.md).
