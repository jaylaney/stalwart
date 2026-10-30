# Review of the Stalwart 1.0.1 upgrade design and plan, revision 5

Reviewed on 2026-10-30 against `2a3d9ea14` on `stalwart-1-upgrade-plan`, fork `main` at
`9a7574203`, `v1.0.1` (`23b69755c`), the revision 4 review (R4-H1 to R4-L6) and the binding
specs. Scope: only the lines revision 5 changed (`git diff d6760e8a0 2a3d9ea14`) in the design,
the plan and the calendar spec (revision 8). Jay's two answers are taken as given. No build
or test was run.

Line numbers refer to the files at `2a3d9ea14`: the design
`docs/superpowers/specs/2026-10-27-stalwart-1-upgrade-design.md`, the plan
`docs/superpowers/plans/2026-10-27-stalwart-1-upgrade.md` and the calendar spec
`docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md`.

## Summary

| Severity | Count |
|---|---:|
| Critical | 0 |
| High | 0 |
| Medium | 4 |
| Low | 5 |

Revision 4 findings: 10 resolved, 1 partly resolved, 0 not resolved.

The policy-version-2 mechanism works against the source: 1.0.1 derives the two flags from
the stored tree on every full write (`refresh_from_content`, reached by `GroupwareWrite::full`
from every writer that remains for key accounts), `ICalendar::privacy()` reads `CLASS` by
name and value from the scheduling components, the resource cache and the swap tier carry
the row's flags, and every DAV reader consumes those flags. Nothing upstream reads needs a
property that stays sealed. `may_be_set_by` runs on the submitted request tree and
`set_privacy` on the incoming iTIP content, both before sealing; `into_private_view` runs on
unsealed content. What is wrong is in the carrying-out: `POLICY_VERSION` is also the key-wrap
envelope byte and is bound into the AEAD associated data, so "the unseal path accepts 1 and 2"
cannot be done as written (R5-M1); the value-conditional allowlist has five name-only
predicate sites and the plan names two (R5-M2); the calendar spec's sections 7, 10, 11 and
12 still describe one version and a three-level allowlist (R5-M3); and PROPPATCH edits the
stored container in `prepare`, a third unseal site the plan omits (R5-M4). The dead-property
wrapper passes the trusted parse as specified; passing the checked validator too needs the
structured DAV-value encoding, which is deferred to the interface sheet (R5-L2).

## Verdicts on the revision 4 findings

| Finding | Verdict | What is left |
|---|---|---|
| R4-H1 privacy flags vs keyless readers | resolved | Field-policy version 2 puts the flags where upstream reads them, with no fork code in the flag path. Carried out with the defects in R5-M1, R5-M2, R5-M3 and R5-L1. |
| R4-M1 wrapper must parse as a container | resolved | Header with `MetadataKinds::DAV`, one `Dav` entry; the value encoding is deferred to the interface sheet, see R5-L2 for the constraint it must meet. |
| R4-M2 seal and unseal sites | partly resolved | The seal site and two unseal sites are right; PROPPATCH's `prepare` reads and edits the stored container itself and is not listed (R5-M4). |
| R4-M3 COPY/MOVE re-key | resolved | In `za_copy_move_event`'s path, with COPY and MOVE round trips. |
| R4-M4 swap tier fixed at boot | resolved | `cache_swap_tests` boots one server per tier through a `TestServerBuilder` bootstrap option (`with_object` already exists for registry objects, `tests/src/utils/server.rs:217`); its own CI step and matrix row. |
| R4-L1 `x:IndexQueueStatus/set` resets | resolved | Reset for transient failures, delete the document or account for permanent ones; the test uses a fail-once hook. |
| R4-L2 C6 and the alarm flags | resolved | C6 lists the alarm flags and, from version 2, the classification flags. |
| R4-L3 `Private` cannot exist | resolved | Stated; leak rule tightened; Email added. |
| R4-L4 presence rewrite | resolved | PROPPATCH that flips a presence bit added to the split test. |
| R4-L5 exit region | resolved | Defined as the region whose start and end both lie on the closing brace. |
| R4-L6 keyless paths untested | resolved | Keyless reads, deletes, destruction and quota over sealed containers. |

## The policy-version-2 mechanism against 1.0.1

What the source confirms:

- The flags are set in one place, `refresh_from_content`
  (`v1.0.1:crates/groupware/src/calendar/index.rs:247`,
  `self.flags = content.data.event.privacy().apply_to_flags(self.flags)`), called by
  `GroupwareWrite::full` (`common/src/storage/index/split.rs:100-104`) and so by
  `CalendarEvent::insert` and `update_full`. The writers that reach it for a key account are
  DAV PUT (`dav/src/calendar/update.rs:596`), DAV COPY/MOVE, iTIP ingest
  (`groupware/src/calendar/itip.rs:693, 977`) and the fork's drain, which mirrors it.
  `update_meta` does not recompute, so PROPPATCH and the presence rewrite keep the last full
  write's bits. `ShareRevokeCleanup`'s `update_full` is skipped for key owners (revision 4),
  and JMAP `CalendarEvent/set` is refused.
- `ICalendar::privacy()` filters scheduling components, takes each `CLASS` entry's first
  value, maps `Classification(Public)` to public, `Classification(Confidential)` to secret
  and anything else to private, and keeps the maximum (`privacy.rs:131-140`). calcard parses
  the three standard tokens to `ICalendarValue::Classification` and any other token to text
  (`calcard/src/icalendar/parser.rs:88-89`, `map_or_text`). So a visible standard `CLASS`
  yields the right flags, and a sealed non-standard one yields public: the recorded limit.
- The readers consume stored flags only: the DAV resource cache's
  `GroupwareResourceMetadata::CalendarEvent { flags }` is built from the row
  (`common/src/storage/dav/store.rs:271-300`), the swap tier persists it, and `itip.rs:854`
  reads the row directly.
- `assert_event_privacy_allowed(&ical, viewer)` → `may_be_set_by` runs on the submitted
  request tree (`dav/src/calendar/update.rs:179, 651`), and `set_privacy` on the incoming
  content in iTIP ingest (`itip.rs:854-875`), both before the fork seals.
  `into_private_view` runs on `CalendarEventContent` after the read (`dav/src/calendar/mod.rs:343-360`),
  where the fork has unsealed. `hidden.rs` (`EVENT_HIDE_ATTENDEES`) is a JMAP-set flag
  (`rights.rs:60`), untouched by this change.

## New findings

### Medium

#### R5-M1. `POLICY_VERSION` is also the key-wrap envelope byte and is bound into the AEAD data; "accepts 1 and 2" cannot be carried out as written

- **Where:** design "Privacy flags", Change (lines 329–331: "`POLICY_VERSION = 2`. Writers
  record 2. The unseal path accepts 1 and 2 … but the version check stays"); plan Task 4
  (lines 452–453); calendar spec line 37 ("until then the fork writes version 1").
- **Evidence:** `POLICY_VERSION` (`crates/groupware/src/calendar/seal/policy.rs:10`) is the
  first byte of every key-wrap envelope, checked for equality: `[POLICY_VERSION, WRAP_MK, …]`
  and `WRAP_PK` in `seal/tree.rs:186-201, 216-235`, `WRAP_CK` in `seal/domain.rs:152, 219-225`,
  `WRAP_PK` again in `seal/item.rs:269`, each with
  `[version, ..] if *version != POLICY_VERSION => Err(SealError::Policy(*version))`. It is
  also the last byte of the tree AAD (`tree_aad`, `tree.rs:103-113`) and of the timezone AAD
  (calendar spec lines 578, 628), and the bundle records no version of its own: the
  carrier is base64 of `format || nonce || ciphertext` (`tree.rs:258-262`), and
  `unseal_component` opens it with `tree_aad(…)` recomputed from the constant
  (`tree.rs:414-423, 453-461`). A version-1 bundle therefore cannot be opened once the
  constant is 2, and "accepts 1 and 2" would need a second AAD attempt per component, which
  nothing in the format supports. Bumping the constant also changes the wire byte of the
  pin-record and collection-key wraps that the key-pinning and sharing specs describe.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** choose one and state it: (a) split the constants, a new
  `FIELD_POLICY_VERSION = 2` used only in `tree_aad`, the timezone AAD and the preferences
  bundle, with the envelope byte staying 1; or (b) bump `POLICY_VERSION` to 2 everywhere and
  accept only 2, since a fresh 1.0 store holds nothing else. Under either, delete "accepts 1
  and 2". Spec section 7 must say which byte changed (R5-M3).

#### R5-M2. The value-conditional entry has five name-only predicate sites; the plan names two

- **Where:** plan Task 4 (lines 449–457: "make `CLASS` visible in `seal/policy.rs` … Update
  the allowlist units … and the leak scanner's allowlist"); design line 329.
- **Evidence:** `is_visible_property(component, name)` takes no value
  (`seal/policy.rs:21`). It is called at `seal/tree.rs:366` (the seal, which has the entry
  and its values in hand), `seal/tree.rs:482` (the round-trip test's `visible_only`),
  `seal/event.rs:435` (the stray-plaintext check), `sharing/format.rs:569` (the conversion
  check) and `tests/src/za/leak.rs:457` (the scanner). A value-conditional rule added to
  `policy.rs` alone changes none of them; a subagent that edits the two named sites leaves
  `event.rs:435` and `format.rs:569` treating a visible `CLASS` as a leak or a conversion
  failure.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** replace the predicate's signature with an entry-aware one,
  `is_visible_entry(component, &ICalendarEntry) -> bool` (name, then value for `CLASS`:
  `matches!(entry.values.first(), Some(ICalendarValue::Classification(_)))`), so every
  caller changes by construction, and list the five sites in Task 4.

#### R5-M3. Calendar spec revision 8 edits sections 2 and 6 only; sections 7, 10, 11 and 12 still describe one version and a three-level allowlist

- **Where:** calendar spec lines 1–37, 120–129, 488–513, 540–544 (edited); lines 549–552,
  578–579, 608, 628, 653, 828, 880, 901–902 (unchanged).
- **Evidence:** section 12 invariant 3 still reads "Visibility is an allowlist at component,
  property and parameter level … Unknown items are sealed" (line 901), and section 11
  requires "visible properties and parameters match the allowlists exactly" (lines 828,
  880); version 2 adds a value level for one property. Section 7 says the policy version is
  "recorded with each sealed object" (549), names it as a byte of `X-ZA-KEY` (579) and of
  the preferences bundle (608) and as AAD input (578, 628); section 7/10 treats "an unknown
  policy version" as an error (653). Revision 8 changes what the version means without
  saying which of these bytes change (R5-M1) or that a value level now exists.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** amend 12.3 ("…at component, property and parameter level, and at
  value level for `CLASS`"), 11's two allowlist sentences, and 7's version paragraph (which
  byte carries 2 and that every object the port writes carries it), in the same revision 8.

#### R5-M4. PROPPATCH edits the stored container in `prepare`, an unseal site the plan omits

- **Where:** plan Task 4, Sites (lines 407–412); design lines 199–204.
- **Evidence:** `DeadPatch::prepare` reads the stored container
  (`v1.0.1:crates/dav/src/common/dead/patch.rs:132`, `metadata_container(…)`), takes
  `current.view()` and builds the edit from it (`MetadataBuilder::from_view`, lines 137–141),
  applies `set_dav`/`remove_dav` to that builder, then encodes `next` (line 185) and calls
  `into_write` (line 230). With the stored bytes being the sealed wrapper, the builder starts
  from the wrapper's one fork-reserved entry, adds the new plaintext entries beside it, and
  the seal of `next` then wraps a mixture; a removal of an existing property finds nothing
  to remove. The plan's seal site is right; the unseal of `current` in `prepare` is missing.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** add "unseal `current` in `prepare` before `MetadataBuilder::from_view`"
  to the Sites bullet, and a PROPPATCH-after-PROPPATCH round trip (set, then remove, then
  PROPFIND) to the tests.

### Low

#### R5-L1. "Fourteen DAV sites" undercounts the readers

- **Where:** design line 315.
- **Evidence:** beyond the fourteen `from_flags`/`EVENT_SECRET` lines,
  `assert_event_privacy_access(flags, viewer)` is called from
  `v1.0.1:crates/dav/src/calendar/{copy_move.rs:113,327, delete.rs:203, proppatch.rs:123,
  update.rs:178}`, all on stored flags.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** "every DAV site that reads them".

#### R5-L2. The wrapper's value must be a structured DAV value to pass the checked validator

- **Where:** design Wrapper (lines 189–198); plan Task 4 Wrapper (lines 399–406).
- **Evidence:** the trusted path only skips a length-prefixed blob
  (`dav_value_span`, `v1.0.1:crates/types/src/metadata/xml/view.rs:16-23`), so "version byte,
  then the ciphertext" passes it. The checked validator requires the structured form: a
  length, a flag byte `0` or `VALUE_HAS_LANG`, attributes, children, then XML text
  (`validate_dav_value`, lines 68–80; `is_xml_text`). At 1.0.1 the checked container
  validator (`MetadataView::new` → `validate`, `view.rs:72-108, 169-171`) has no production
  caller, and `DavValueView::parse` is used only for lock owners (`dav/src/common/lock.rs:855`),
  so the trusted form is what the port needs; the text's "both parsers" is satisfiable only
  by the structured form with the payload as base64 text.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** fix the encoding in the interface sheet as "a DAV value with no
  language, no attributes and no children whose text is base64 of
  `version || ciphertext`", and note that the checked path is unused at 1.0.1.

#### R5-L3. The stored flags are per document, the visible `CLASS` values per component

- **Where:** calendar spec lines 122–123 ("the classification of each event, task and
  journal component"); design lines 332–333.
- **Evidence:** `privacy()` keeps the maximum over the scheduling components
  (`privacy.rs:138-139`), so a recurrence override with `CLASS:PRIVATE` under a public
  master marks the whole document private; the tree itself shows each component's value.
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** "each component's classification in the tree, and the most
  restrictive of them in the event's flags".

#### R5-L4. Where the classification also lands

- **Where:** calendar spec lines 120–123 (the Visible list).
- **Evidence:** the two bits sit in the metadata row, the DAV resource cache, the swap
  tier's persisted resources (`cache/swap/resources.rs`) and, if `full_index_values`
  indexes the flags, the index subspace. The Visible list names the tree only.
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** add "and in the flags of the event's row, resource cache and
  snapshots" to the version-2 sentence.

#### R5-L5. The drain's mirror must carry upstream's privacy preservation on inbound updates

- **Where:** design lines 338–339; plan Task 4, drain bullet (unchanged text, "re-read
  both against 1.0.1's `itip_ingest` and `commit_itip_merge`").
- **Evidence:** on an inbound update, upstream restores the stored classification into the
  incoming content when it differs (`v1.0.1:crates/groupware/src/calendar/itip.rs:854-875`,
  `set_privacy(privacy)`), so an attendee's reply cannot change an organizer's `CLASS`. The
  fork's drain mirrors `commit_itip_merge` by hand; the generic re-read instruction does not
  name this block, and the store-scan test (plan line 463) lists "a drain import and merge"
  without a differing incoming `CLASS`.
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** name `itip.rs:854-875` in the drain bullet and add "an inbound
  update whose `CLASS` differs from the stored one" to the store-scan cases.

## Questions for Jay

None beyond the choice in R5-M1, which the implementer can make if the design says which
option; (b) is the smaller change on a fresh store.

## Verified in revision 5

- The dispositions table rows R4-H1 to R4-L6 match the text they cite.
- `cache_swap_tests` as its own suite function, matrix row and CI step; the tier is built
  once (`config/inner.rs:125`) and `cache/reload.rs` does not rebuild it.
- `x:IndexQueueStatus/set` resets the status key (`index_queue.rs:191-240`); the test uses
  a fail-once hook.
- The exit-region wording in the design and the plan (Tasks 1 and 7).
- `DeadContainers::copy` and the re-key in `za_copy_move_event`'s path; cross-account copies
  refused (calendar spec section 8).
- DAV writes only `Shared` (`dead/patch.rs:190`; no DAV caller of `into_private_write`).
- `ICalendarClassification::{Public, Private, Confidential}` and calcard's `map_or_text`.
- `EventPrivacy::from_value`, `may_be_set_by` (`viewer.is_owner() || self.is_public()`),
  `set_privacy`, `into_private_view`, and their call sites.
- The ownership table no longer lists `common/src/storage/metadata/` under Task 4.
