# Stalwart 0.16.27 update design and plan review

Originally reviewed 2026-10-27 on `main` at
`f9bc65b3d4f4be5acfae4737eb5fb3957d6f0857`; reconciled against source and corrected documents
on 2026-10-28. The original review identified one P2, six P3s and three nits. The confirmed
plan gaps are addressed in the current drafts; several original claims are corrected below.
This is documentation disposition, not implementation or runtime acceptance.

## Reviewed versions and evidence boundary

- [Current design](2026-10-27-stalwart-01627-upgrade-design.md), SHA-256
  `8470ed525dc84290a1279f1df5e82a67c2d2cf71c9037e03a83e30fd35190570`.
- [Current plan](../plans/2026-10-27-stalwart-01627-upgrade.md), SHA-256
  `b111cda1d45c03daf4b4d09ed791e13f3a901ddddac1b6079f5a6473616fcc3e`.
- Original design SHA-256:
  `b4d9e18599a0e926d3ee5f2e1f3b363d9ec4deec63b5bffe0604c9bb153b3647`.
- Original plan SHA-256:
  `151ff6ba3080119a20331045a0c3fc942afed02b5beea0a484da92b3bd44b613`.
- Fork source remains `f9bc65b3d4f4be5acfae4737eb5fb3957d6f0857`; old upstream base is
  `18f4e9505027d790fd7e9fdad7f922be6719101d`; target `v0.16.27` peels to
  `aa47d8ef7bfba7200d3293198ac2592747b20e73`.
- The original review inspected the upstream delta, a temporary dry merge and parsed
  lockfiles. Reconciliation inspected the same pinned source, actual test dispatch,
  dependency timezone resolver and release fixtures. No product code was edited, and no
  build, test, working-tree merge, CI or live-store run establishes these conclusions.

## Dispositions

| Original item | Disposition in corrected documents |
|---|---|
| P2 key-mode IMAP expectation | Accepted; Task 1 audits variants and Task 2 updates ALL/SAVE coverage. |
| P3 dirty-tree merge | Accepted; Task 2 commits the red regression and requires a clean merge start. |
| P3 crypto/lockfile inventory | Qualified; enumerate real dependency movement, preserve the full fork graph and record the chosen assurance scope. |
| P3 timezone control and REPLY behavior | Accepted with bounded wording; recognized missing zones may be generated, and a REPLY supplies only missing referenced definitions. |
| P3 time-range assertion | Accepted with a corrected fixture/resolver explanation; no arbitrary-offset or background-content-index claim. |
| P3 push test expansion | Narrowed; retain existing SMTP privacy controls and plain/key EventSource coverage. |
| P3 edited-crate unit gates | Accepted; Task 4 includes IMAP/JMAP/DAV/DAV-proto unit harnesses. |
| Release-fixture nit | Partly rejected; the real-manifest test needs alignment, while the self-contained product-verifier fixture may stay unchanged. |
| README remote and whitespace nits | Accepted; Task 3 fixes the example and Task 4 checks committed changes over an explicit range. |

## Confirmed IMAP gap

The original plan omitted `tests/src/imap/za_variants.rs`. Key mode dispatches
`za_variants::search` ([dispatch](../../../tests/src/imap/mod.rs:260)), whose ten-message
assertion still expects `COUNT 10 MIN 1 MAX 10 ALL 1,10`
([variant](../../../tests/src/imap/za_variants.rs:107)). The .27 plain test changes that
expectation to `ALL 1:10`, matching the corrected search implementation.

The corrected Task 2 names the variant, updates the expectation and adds SAVE/MIN/MAX/ALL
using the existing permitted keyword set: Flag_000, Flag_002, Flag_003 and Flag_005. A later
`UID SEARCH $` must return `1 3 4 6`. FROM/SUBJECT criteria remain refused for key accounts.
Task 1 also requires auditing changed upstream test bodies against their variants, as
required by ruling R44 in `CLAUDE.md`.

This is a source-predicted failure of the unadapted merge, not an executed failing test.
The key-mode IMAP gate remains required after implementation.

## Clean merge and lockfile reconciliation

Task 2 now records the .26 behavioral red receipt at a committed regression checkpoint,
then requires a clean tracked worktree/index before `git merge --no-commit`. This makes the
baseline reproducible and avoids relying on abort to recover uncommitted test work.

The original lockfile acceptance rule was too narrow. Parsing the starting fork against
upstream .26 shows differences in 12 package names: `argon2`, `chacha20`,
`chacha20poly1305`, `common`, `dav`, `email`, `groupware`, `http`, `jmap`, `stalwart`,
`tests` and `vault`. These include vault consumers and feature-induced zeroize edges;
they are not merely two workspace package entries.

The corrected documents permit using .27's lockfile as a baseline, followed by Cargo
resolution of the retained fork manifests/features without a blanket update. Review the
normalized graph and every additional version/edge change. Do not discard legitimate
fork dependencies to satisfy a two-package comparison, or take a whole handler file as a
conflict resolution.

The dependency movement includes `sha2` 0.11.0 to 0.11.1, `inout` 0.2.2 to 0.2.3,
rustls/aws-lc, sequoia-openpgp, Tokio, dns-update 0.5 to 0.6 and pest's stacker/psm additions.
The original plan already asked for dependency movement; the correction makes the important
chains and reconciliation acceptance explicit.

The original crypto-test description was also wrong. The RFC7748 Alice public key is an
input to literal SHA-256 fingerprint expectations, not an X25519 known-answer computation
([keys.rs](../../../crates/vault/src/keys.rs:703)). Existing tests therefore detect some
SHA-256 output changes. No fixed HKDF/Argon2/AEAD known-answer tests were found, so those
checks do not prove complete cross-version crypto compatibility.

For the undeployed preview, the design proposes dependency/feature audit plus existing
crypto units and functional gates, with residual patch-dependency risk recorded. No old-store
compatibility contract is claimed. A cross-version fixture or additional KDF/AEAD vectors
would be extra assurance, not a required migration gate in this scope.

## Timezone correction and required regression

The .27 merge copies incoming definitions only when their TZID is referenced in the merged
calendar and lacks an existing same-ID definition. It then calls `add_missing_timezones()`.
Recognized missing TZIDs can gain generated definitions; an unresolvable custom TZID is not
necessarily synthesized. The no-new-timezone control must already define every referenced
TZID. Stored size/ETag can legitimately change when definitions are added, and drain's
size/quota checks remain after merging.

An authenticated participant REPLY that produces merge actions may supply such a missing,
referenced definition. It does not replace an existing same-ID definition. Token, participant
and stale-reply checks remain in the fork drain; the design records this bounded inherited
behavior without changing scheduling authorization.

The original suggested arbitrary custom offset fixture cannot provide its claimed time-range
red/green result. Calcard 0.3.14's `src/icalendar/timezone.rs:84-111` resolves timezone names
through parsed TZID, X-LIC-LOCATION or a mapped Microsoft CDO ID. It does not interpret arbitrary
STANDARD/DAYLIGHT observances to resolve an otherwise unknown custom TZID. Resolution falls
back when no recognized name/alias exists.

The corrected fixture uses a custom TZID with `X-LIC-LOCATION:Europe/Paris`, a UTC default
and a winter date. Losing the supplied VTIMEZONE loses that alias mapping; importing it
changes resolution. Assert both the decrypted recurrence/time and calendar-query time-range
REPORT behavior: the correct moved-time window matches, and the fallback-time window does not.

The fork visibility policy preserves TZID and X-LIC-LOCATION
([policy.rs](../../../crates/groupware/src/calendar/seal/policy.rs:50)). The drain computes
derived event data before sealing ([drain.rs](../../../crates/groupware/src/calendar/drain.rs:1373)).
This checks persisted derived times/query behavior, not background content indexing:
key-account/sealed-calendar content indexing remains skipped. Record the developer guide's
visibility consistency check without claiming a new derived-value type was introduced.

## Notification scope

All five changed mail write sites broadcast content-free `PushNotification::StateChange`;
EmailDelivery is another state type, not a route into the EmailPush content builder.
The HTTP builder ignores StateChange while constructing content objects
([http.rs](../../../crates/services/src/state_manager/http.rs:73)). The fork's content-push
refusal/conversion defenses remain in
[email_push.rs](../../../crates/services/src/state_manager/email_push.rs:67) and
[push.rs](../../../crates/services/src/state_manager/push.rs:369).

The original plan's broad mock-push privacy extension is removed. Retain the existing SMTP
ordinary/key content-push control and the supplied import EventSource EmailDelivery assertion,
which runs in both modes. Ordinary APPEND/import is not a positive content-push control.

The original review's claim that additional state assertions could only pass was too strong:
they could detect missing or misrouted notifications. The selected scope uses existing
coverage and source review, rather than adding per-write mock-push round trips for a
content-filter bypass these sites cannot create.

## Qualification and identity corrections

Task 4 now includes `cargo test -p imap -p jmap -p dav -p dav-proto` for the package unit
harnesses. Existing integration suites compile these libraries as dependencies but do not run
their `#[cfg(test)]` units. The named filter/sort and fork DAV/JMAP classification cases
exist; this review makes no claim about command success or measured execution cost.

The original claim that both release fixture modules were self-contained is false.
`test_release_metadata.py` copies the real shipping manifest into its fixture
([line 88](../../../scripts/tests/test_release_metadata.py:88)) and compares it with its
fixture VERSION. Task 3 must align that version when the product changes to beta.2.
`test_verify_product.py` is self-contained and may remain unchanged; if its fixture version
is changed, its mismatched-tag controls must still use a different tag.

Task 3 explicitly replaces README-dev's nonexistent upstream-remote merge example with
URL fetch and verified-tag merge. Task 4 uses the full starting fork SHA in the committed
`git diff --check` range, plus working/index checks while editing. A bare command after
commit would miss those committed changes.

Local macOS identity validation remains separate from the Linux provenance CLI: use
load_metadata/verify_checkout/validate_identity locally, and reserve the CLI's fixed Linux
artifact metadata and ELF inspection for the authorized Linux workflow. No publication,
CI dispatch, deployment or implementation is authorized by these documentation corrections.

## Unchanged source facts

- The target delta is 8 commits, 53 paths, +717/-265 against .26; the three conflict paths
  are Cargo.lock, crates/main/Cargo.toml and the DAV common propfind import.
- The specified combined Cow/BTreeMap/Arc import is correct. The property-selection hunks
  auto-merge; fork admission, memoization and stored ETag behavior must remain.
- Three iTIP callers require the incoming calendar: `calendar/itip.rs` uses `&itip`,
  `calendar/drain.rs` uses `&tree`, and the retained WebDAV helper uses `&message.message`.
- `map_search_results` has one local caller, updated together with its collect_all argument.
- Calcard 0.3.14 and rkyv 0.8.18 stay fixed. The target does not alter the reserved numeric
  fields, key-account flag or vanished collection. This is not runtime format qualification.
- The new WebUI guard and OAuth unit cases remain public. The .27 tests crate and workflow
  remain available; the separate 1.0 private-test transition is not part of this update.
- The beta.2 naming proposal follows the release policy. Actual candidate results, Linux
  artifact provenance and native-client status remain future implementation/release gates.
